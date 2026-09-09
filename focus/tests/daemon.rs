// Protocol tests: parsing a command line, and the bytes that go on the
// wire. Both are pure, so they run anywhere.

use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use bstr::BStr;
use focus::daemon::{Request, parse_args};

// Snapshots live next to the test. `UPDATE_SNAPSHOTS=1 cargo test`
// rewrites them; without it a change is a failure.
fn check_snapshot(name: &str, snapshot: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/snapshots")
        .join(name);
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        std::fs::write(&path, snapshot).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_default();
    assert_eq!(
        snapshot, expected,
        "{name} changed; rerun with UPDATE_SNAPSHOTS=1 to accept"
    );
}

fn args(args: &[&str]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}

fn non_utf8_path() -> PathBuf {
    PathBuf::from(OsString::from_vec(vec![b'/', 0xff, 0xfe, b'x']))
}

// The rule worth pinning down: a relative path is resolved against the
// *client's* cwd, since the daemon's own cwd is `/` and means nothing.
#[test]
fn parse_args_snapshot() {
    let cases: &[(&str, &[&str])] = &[
        ("/home/j", &[]),
        ("/home/j", &["notes.txt"]),
        ("/home/j", &["../other/notes.txt"]),
        ("/tmp", &["notes.txt"]),
        ("/home/j", &["/abs/notes.txt"]),
        ("/home/j", &["--launcher"]),
        ("/home/j", &["--quit"]),
        ("/home/j", &["--replace"]),
        ("/home/j", &["--replace", "--foreground"]),
        ("/home/j", &["--no-wait"]),
        ("/home/j", &["--no-wait", "notes.txt"]),
        ("/home/j", &["--daemon", "--no-wait"]),
        ("/home/j", &["--replace", "notes.txt"]),
        ("/home/j", &["--foreground", "--launcher"]),
        ("/home/j", &["--daemon"]),
        ("/home/j", &["--daemon", "--replace"]),
        ("/home/j", &["--daemon", "notes.txt"]),
        ("/home/j", &["notes.txt", "other.txt"]),
        ("/home/j", &["--launcher", "--quit"]),
        ("/home/j", &["--nope"]),
        ("/home/j", &["-"]),
    ];
    let mut snapshot = String::new();
    for (cwd, argv) in cases {
        snapshot.push_str(&format!(
            "{} $ focus {}\n  {:?}\n",
            cwd,
            argv.join(" "),
            parse_args(&args(argv), Path::new(cwd))
        ));
    }
    check_snapshot("parse_args.txt", &snapshot);
}

#[test]
fn encode_snapshot() {
    let cases = [
        Request::Scratch,
        Request::File(PathBuf::from("/home/j/notes.txt")),
        Request::File(PathBuf::from("/a/with\nnewline")),
        Request::File(non_utf8_path()),
        Request::Launcher,
        Request::Quit,
    ];
    let mut snapshot = String::new();
    for request in cases {
        snapshot.push_str(&format!(
            "{:?}\n  {:?}\n",
            request,
            BStr::new(&request.encode())
        ));
    }
    check_snapshot("encode.txt", &snapshot);
}

#[test]
fn decode_roundtrips() {
    let cases = [
        Request::Scratch,
        Request::Launcher,
        Request::Quit,
        Request::File(PathBuf::from("/a/b.rs")),
        // Paths are arbitrary bytes: no framing means no byte is special.
        Request::File(PathBuf::from("/a/with\nnewline")),
        Request::File(PathBuf::from("/a/with space")),
        Request::File(PathBuf::from("/a/--launcher")),
        Request::File(non_utf8_path()),
        Request::File(PathBuf::from(format!("/{}", "deep/".repeat(2000)))),
    ];
    for request in cases {
        assert_eq!(Request::decode(&request.encode()), Ok(request.clone()));
    }
}

// Decode parses bytes off a socket, so every input has to produce an
// answer rather than a panic.
#[test]
fn decode_rejects_garbage() {
    let cases: &[&[u8]] = &[
        b"--nope",
        b"--launcher\n",
        b"--quit ",
        b" --quit",
        b"relative/path",
        b"-",
        b"--",
        b"\xff\xfe",
        b"\0",
        b"\0/a",
    ];
    for bytes in cases {
        assert!(
            Request::decode(bytes).is_err(),
            "expected an error for {:?}",
            bytes
        );
    }
    // Big inputs are answered, not feared: junk is rejected, a long path
    // is accepted.
    assert!(Request::decode(&vec![b'x'; 1 << 20]).is_err());
    assert!(Request::decode(&[b"/".as_slice(), &vec![b'x'; 1 << 20]].concat()).is_ok());
}

// Client tests. Real sockets, real flock, real files - only the daemon is
// a stub, so everything up to the exec is covered.

use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Barrier};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use focus::daemon::{
    Cli, REPLY_OK, Started, connect_or_start, listener_from_fd, serve, socket_path, wait_for_close,
};

// A fresh runtime dir per test, so tests do not share a socket. Named
// after the test rather than randomly, so a leftover dir is obvious.
fn runtime_dir(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("focus-daemon-test-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn cli(request: Request) -> Cli {
    Cli {
        request,
        replace: false,
        foreground: false,
        daemon: false,
        no_wait: false,
    }
}

// Stands in for the daemon: serves `count` requests and hands them back.
fn stub_daemon(listener: UnixListener, count: usize) -> JoinHandle<Vec<Request>> {
    thread::spawn(move || {
        let mut requests = Vec::new();
        for stream in listener.incoming().take(count) {
            let mut stream = stream.unwrap();
            let mut bytes = Vec::new();
            stream.read_to_end(&mut bytes).unwrap();
            requests.push(Request::decode(&bytes).unwrap());
            // A client that queued its request and left is gone by now,
            // so the reply can fail with EPIPE. Never fatal.
            let _ = stream.write_all(REPLY_OK);
        }
        requests
    })
}

fn listening(started: Started) -> UnixListener {
    match started {
        Started::Listening { listener, .. } => listener,
        _ => panic!("expected to start a daemon, but one was already running"),
    }
}

fn assert_sent(started: Started) {
    match started {
        Started::Sent(_) => {}
        Started::NothingToDo => panic!("expected the request to be sent, but nothing was"),
        Started::Listening { .. } => {
            panic!("expected an existing daemon, but a new one was started")
        }
    }
}

#[test]
fn request_goes_to_a_running_daemon() {
    let dir = runtime_dir("running");
    let stub = stub_daemon(UnixListener::bind(socket_path(&dir)).unwrap(), 1);

    let request = Request::File(PathBuf::from("/a/b.rs"));
    assert_sent(connect_or_start(&dir, &cli(request.clone())).unwrap());

    assert_eq!(stub.join().unwrap(), [request]);
}

#[test]
fn cold_start_binds_and_queues_the_request() {
    let dir = runtime_dir("cold");

    // Nothing is listening, so the client binds the socket itself and
    // leaves the request queued on it for the daemon it is about to
    // start - no waiting for that daemon to be ready.
    let listener = listening(connect_or_start(&dir, &cli(Request::Launcher)).unwrap());

    assert_eq!(
        stub_daemon(listener, 1).join().unwrap(),
        [Request::Launcher]
    );
}

#[test]
fn a_stale_socket_is_replaced() {
    let dir = runtime_dir("stale");
    // A daemon that crashed: the socket file outlives the process, since
    // daemons never unlink it.
    drop(UnixListener::bind(socket_path(&dir)).unwrap());

    let listener = listening(connect_or_start(&dir, &cli(Request::Scratch)).unwrap());

    assert_eq!(stub_daemon(listener, 1).join().unwrap(), [Request::Scratch]);
}

#[test]
fn a_plain_file_in_the_way_is_replaced() {
    let dir = runtime_dir("plain-file");
    std::fs::write(socket_path(&dir), b"not a socket").unwrap();

    let listener = listening(connect_or_start(&dir, &cli(Request::Scratch)).unwrap());

    assert_eq!(stub_daemon(listener, 1).join().unwrap(), [Request::Scratch]);
}

#[test]
fn replace_quits_the_old_daemon_and_starts_a_new_one() {
    let dir = runtime_dir("replace");
    let old = stub_daemon(UnixListener::bind(socket_path(&dir)).unwrap(), 1);

    let started = connect_or_start(
        &dir,
        &Cli {
            replace: true,
            ..cli(Request::Launcher)
        },
    )
    .unwrap();

    // The old daemon is told to quit, and the request goes to the new
    // listener rather than to it.
    assert_eq!(old.join().unwrap(), [Request::Quit]);
    assert_eq!(
        stub_daemon(listening(started), 1).join().unwrap(),
        [Request::Launcher]
    );
}

#[test]
fn quit_reaches_a_running_daemon() {
    let dir = runtime_dir("quit-running");
    let stub = stub_daemon(UnixListener::bind(socket_path(&dir)).unwrap(), 1);

    assert_sent(connect_or_start(&dir, &cli(Request::Quit)).unwrap());

    assert_eq!(stub.join().unwrap(), [Request::Quit]);
}

#[test]
fn quit_without_a_daemon_starts_nothing() {
    let dir = runtime_dir("quit-cold");

    assert!(matches!(
        connect_or_start(&dir, &cli(Request::Quit)).unwrap(),
        Started::NothingToDo
    ));

    assert!(!socket_path(&dir).exists());
}

// The race the lock file exists for: without it, several clients each
// find no daemon and each bind, and all but the last end up talking to an
// orphaned socket. One round catches that most of the time; a handful of
// rounds catches it essentially always.
#[test]
fn concurrent_clients_start_exactly_one_daemon() {
    for round in 0..5 {
        let dir = runtime_dir(&format!("concurrent-{round}"));
        race_one_round(&dir);
    }
}

fn race_one_round(dir: &Path) {
    const CLIENTS: usize = 8;

    // Without the barrier the threads mostly start far enough apart that
    // the first one wins by luck rather than by locking.
    let barrier = Arc::new(Barrier::new(CLIENTS));
    let clients: Vec<_> = (0..CLIENTS)
        .map(|_| {
            let dir = dir.to_path_buf();
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                match connect_or_start(&dir, &cli(Request::Scratch)).unwrap() {
                    // Whoever binds is responsible for serving, so it
                    // starts the daemon before returning, exactly as the
                    // real client does.
                    Started::Listening { listener, .. } => Some(stub_daemon(listener, CLIENTS)),
                    _ => None,
                }
            })
        })
        .collect();

    let daemons: Vec<_> = clients
        .into_iter()
        .filter_map(|client| client.join().unwrap())
        .collect();
    assert_eq!(daemons.len(), 1);
    let requests = daemons.into_iter().next().unwrap().join().unwrap();
    assert_eq!(requests, vec![Request::Scratch; CLIENTS]);
}

// Daemon-side tests. `serve` takes a sink rather than an
// `EventLoopProxy`, so the whole socket path runs with no winit in sight.

#[test]
fn serve_delivers_requests_to_the_sink() {
    let dir = runtime_dir("serve");
    let (sender, receiver) = mpsc::channel();
    serve(
        UnixListener::bind(socket_path(&dir)).unwrap(),
        move |incoming| sender.send(incoming).unwrap(),
    );

    let request = Request::File(PathBuf::from("/a/b.rs"));
    assert_sent(connect_or_start(&dir, &cli(request.clone())).unwrap());

    assert_eq!(receiver.recv().unwrap().request, request);
}

// A daemon must not be killable by whatever a client happens to write.
#[test]
fn serve_answers_garbage_and_keeps_going() {
    let dir = runtime_dir("serve-garbage");
    let (sender, receiver) = mpsc::channel();
    serve(
        UnixListener::bind(socket_path(&dir)).unwrap(),
        move |incoming| sender.send(incoming).unwrap(),
    );

    let mut stream = UnixStream::connect(socket_path(&dir)).unwrap();
    stream.write_all(b"not a request").unwrap();
    stream.shutdown(std::net::Shutdown::Write).unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).unwrap();
    assert!(reply.starts_with("err "), "{reply:?}");

    // Still serving.
    assert_sent(connect_or_start(&dir, &cli(Request::Launcher)).unwrap());
    assert_eq!(receiver.recv().unwrap().request, Request::Launcher);
}

// The daemon half of the fd handoff: after the exec its listener is just
// a number, and it has to be able to pick it back up.
#[test]
fn a_listener_survives_being_passed_as_an_fd() {
    let dir = runtime_dir("listen-fd");
    let listener = UnixListener::bind(socket_path(&dir)).unwrap();
    // Stands in for the dup2 in `spawn`.
    let fd = unsafe { libc::dup(listener.as_raw_fd()) };
    assert!(fd >= 0);
    drop(listener);

    let recovered = listener_from_fd(fd).unwrap();
    let stub = stub_daemon(recovered, 1);
    assert_sent(connect_or_start(&dir, &cli(Request::Scratch)).unwrap());

    assert_eq!(stub.join().unwrap(), [Request::Scratch]);
}

#[test]
fn a_listener_fd_that_is_not_a_socket_is_rejected() {
    let dir = runtime_dir("listen-fd-bad");
    let file = std::fs::File::create(dir.join("not-a-socket")).unwrap();

    assert!(listener_from_fd(file.as_raw_fd()).is_err());
    // Don't let the failed listener's drop close the file's fd twice.
    std::mem::forget(file);
}

// Waiting is what makes `focus` usable as $EDITOR: the client must block
// until the daemon lets go of the connection, which it does when the
// window closes.
#[test]
fn waiting_blocks_until_the_daemon_closes_the_connection() {
    let dir = runtime_dir("wait");
    let listener = UnixListener::bind(socket_path(&dir)).unwrap();
    let closed = Arc::new(AtomicBool::new(false));

    let daemon = {
        let closed = Arc::clone(&closed);
        thread::spawn(move || {
            let mut stream = listener.incoming().next().unwrap().unwrap();
            let mut bytes = Vec::new();
            // Read only the request, not to EOF: the client is still
            // holding its end open, waiting.
            stream.read_to_end(&mut bytes).unwrap();
            stream.write_all(REPLY_OK).unwrap();
            // Stand in for a window staying open for a while.
            thread::sleep(Duration::from_millis(100));
            closed.store(true, Ordering::SeqCst);
            drop(stream);
        })
    };

    let started = connect_or_start(&dir, &cli(Request::Scratch)).unwrap();
    let Started::Sent(connection) = started else {
        panic!("expected the request to reach the stub daemon");
    };
    // The reply has already been read, so without waiting the client
    // would be free to exit here.
    assert!(!closed.load(Ordering::SeqCst));

    wait_for_close(connection);

    assert!(
        closed.load(Ordering::SeqCst),
        "returned before the daemon closed the connection"
    );
    daemon.join().unwrap();
}

// `focus ./foo` has to mean the shell's ./foo, not the daemon's - the
// daemon's cwd is "/". The client resolves the path before sending, so
// the answer is visible on the wire, with no compositor needed.
#[test]
fn a_relative_path_resolves_against_the_clients_cwd() {
    let dir = runtime_dir("relative-cwd");
    let work = dir.join("work/sub");
    std::fs::create_dir_all(&work).unwrap();
    // /tmp can itself be a symlink, and the child's getcwd reports the
    // resolved path.
    let work = std::fs::canonicalize(&work).unwrap();
    let stub = stub_daemon(UnixListener::bind(socket_path(&dir)).unwrap(), 3);

    for arg in ["./foo", "foo", "../sub/foo"] {
        let status = Command::new(env!("CARGO_BIN_EXE_focus"))
            .arg(arg)
            .current_dir(&work)
            .env("XDG_RUNTIME_DIR", &dir)
            .status()
            .unwrap();
        assert!(status.success(), "focus {arg} failed");
    }

    // Every spelling names the same file under the client's cwd, once the
    // daemon canonicalizes it as `buffer::from_file` does.
    for request in stub.join().unwrap() {
        let Request::File(path) = request else {
            panic!("expected a file request, got {request:?}");
        };
        assert!(path.is_absolute(), "{path:?} is not absolute");
        assert_eq!(focus::chrome::canonical_path(&path), work.join("foo"));
    }
}
