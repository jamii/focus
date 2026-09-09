// The client/daemon protocol.
//
// One connection carries one request: the client writes the encoded
// request, shuts down its write half, and reads the reply to EOF. The
// daemon reads to EOF, parses, replies, and closes. There is no framing -
// the shutdown is the frame - so a request can hold arbitrary bytes,
// which matters because paths are arbitrary bytes.

use std::ffi::{OsStr, OsString};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::Shutdown;
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::APP_ID;

/// What a client asks the daemon to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// Open a window on a new scratch buffer.
    Scratch,
    /// Open a window on this file. Always absolute: the client resolves
    /// relative paths against its own cwd, because the daemon's is `/`.
    File(PathBuf),
    /// Open a window on the launcher.
    Launcher,
    /// Close every window - autosaving - and exit.
    Quit,
}

const LAUNCHER_ARG: &[u8] = b"--launcher";
const QUIT_ARG: &[u8] = b"--quit";
const REPLACE_ARG: &[u8] = b"--replace";
const FOREGROUND_ARG: &[u8] = b"--foreground";
const DAEMON_ARG: &[u8] = b"--daemon";
const NO_WAIT_ARG: &[u8] = b"--no-wait";

pub const REPLY_OK: &[u8] = b"ok\n";

impl Request {
    pub fn encode(&self) -> Vec<u8> {
        match self {
            Request::Scratch => Vec::new(),
            Request::File(path) => path.as_os_str().as_bytes().to_vec(),
            Request::Launcher => LAUNCHER_ARG.to_vec(),
            Request::Quit => QUIT_ARG.to_vec(),
        }
    }

    /// Parse bytes read off a socket, so: never panic, and reject anything
    /// unrecognized. A file path is told apart from a flag by its leading
    /// `/` - `encode` only ever writes absolute paths.
    pub fn decode(bytes: &[u8]) -> Result<Request, String> {
        match bytes {
            b"" => Ok(Request::Scratch),
            LAUNCHER_ARG => Ok(Request::Launcher),
            QUIT_ARG => Ok(Request::Quit),
            _ if bytes[0] == b'/' => Ok(Request::File(PathBuf::from(OsString::from_vec(
                bytes.to_vec(),
            )))),
            _ => Err(format!(
                "unrecognized request: {:?}",
                String::from_utf8_lossy(bytes)
            )),
        }
    }
}

/// The whole command line. The flags choose which process ends up running
/// the daemon; the request is what gets sent to it either way.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cli {
    pub request: Request,
    /// Quit the running daemon, if any, before starting a new one.
    pub replace: bool,
    /// Run the daemon in this process rather than detaching one, and apply
    /// the request directly. For debugging and profiling.
    pub foreground: bool,
    /// Internal: this process *is* the freshly spawned daemon, and takes
    /// its listener from `FOCUS_LISTEN_FD`.
    pub daemon: bool,
    /// Return as soon as the daemon has the request, rather than waiting
    /// for the window it opens to close.
    ///
    /// Waiting is the default so that `focus` works as `$EDITOR` with no
    /// configuration, and because `focus notes.txt` blocked the terminal
    /// before there was a daemon too. Ctrl-C gets the shell back without
    /// disturbing the window, since the window belongs to the daemon.
    pub no_wait: bool,
}

/// Parse argv (without argv[0]). This is where relative paths are resolved
/// against the *client's* cwd - the daemon's own cwd is `/` and means
/// nothing - so it takes the cwd rather than reading it.
pub fn parse_args(args: &[OsString], cwd: &Path) -> Result<Cli, String> {
    let mut cli = Cli {
        request: Request::Scratch,
        replace: false,
        foreground: false,
        daemon: false,
        no_wait: false,
    };
    let mut request_arg: Option<&OsStr> = None;
    for arg in args {
        let bytes = arg.as_bytes();
        let request = match bytes {
            REPLACE_ARG => {
                cli.replace = true;
                continue;
            }
            FOREGROUND_ARG => {
                cli.foreground = true;
                continue;
            }
            DAEMON_ARG => {
                cli.daemon = true;
                continue;
            }
            NO_WAIT_ARG => {
                cli.no_wait = true;
                continue;
            }
            LAUNCHER_ARG => Request::Launcher,
            QUIT_ARG => Request::Quit,
            _ if bytes.first() == Some(&b'-') => {
                return Err(format!("unknown flag: {:?}", arg));
            }
            _ => Request::File(cwd.join(Path::new(arg))),
        };
        if let Some(previous) = request_arg {
            return Err(format!(
                "expected at most one of a path, --launcher or --quit, got {:?} and {:?}",
                previous, arg
            ));
        }
        request_arg = Some(arg);
        cli.request = request;
    }
    if cli.daemon && (cli.replace || cli.foreground || cli.no_wait || request_arg.is_some()) {
        return Err("--daemon takes no other arguments".to_string());
    }
    Ok(cli)
}

// ---------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------

/// Where the socket and lock live. `XDG_RUNTIME_DIR` is per-user, 0700,
/// and cleared on logout, so a stale socket never outlives a session.
pub fn runtime_dir() -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(format!("/run/user/{}", unsafe { libc::getuid() })),
    }
}

pub fn socket_path(runtime_dir: &Path) -> PathBuf {
    runtime_dir.join(format!("{APP_ID}.sock"))
}

pub fn lock_path(runtime_dir: &Path) -> PathBuf {
    runtime_dir.join(format!("{APP_ID}.lock"))
}

/// What happened to the request.
pub enum Started {
    /// There was nothing to do: `--quit`, with no daemon running.
    NothingToDo,
    /// A daemon was already running and has been sent the request.
    Sent(UnixStream),
    /// There was no daemon. The socket is bound and listening and the
    /// request is already queued on it, waiting to be accepted - so this
    /// listener now has to be served, either by this process or by one
    /// spawned to inherit it.
    Listening {
        listener: UnixListener,
        connection: UnixStream,
    },
}

/// Hand `cli.request` to the daemon, starting one if there is none.
///
/// The whole decision runs under a lock file, so two `focus` invocations
/// racing cannot both decide to start a daemon.
pub fn connect_or_start(runtime_dir: &Path, cli: &Cli) -> std::io::Result<Started> {
    // Dropping the file closes the fd, which releases the lock. Held
    // only until the socket is bound: from then on a racing client
    // connects to it and its request queues in the backlog, whether or
    // not the daemon has started accepting yet.
    let _lock = lock(runtime_dir)?;
    let socket = socket_path(runtime_dir);
    let existing = UnixStream::connect(&socket).ok();

    // `--quit` never starts a daemon: with none running there is nothing
    // to do.
    if cli.request == Request::Quit {
        let Some(mut stream) = existing else {
            return Ok(Started::NothingToDo);
        };
        write_request(&mut stream, &Request::Quit)?;
        read_reply(&stream)?;
        return Ok(Started::Sent(stream));
    }

    if let Some(mut stream) = existing {
        if !cli.replace {
            write_request(&mut stream, &cli.request)?;
            read_reply(&stream)?;
            return Ok(Started::Sent(stream));
        }
        // Replacing: tell the old daemon to go away. We do not wait for
        // it to die - it never unlinks the socket, so its inode is ours
        // to replace right now, and it closes its windows and exits
        // against a socket nobody can reach any more.
        write_request(&mut stream, &Request::Quit)?;
        read_reply(&stream)?;
    }

    // Either nothing was listening (no daemon, or one that crashed and
    // left its socket behind) or we just evicted one.
    match std::fs::remove_file(&socket) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let listener = UnixListener::bind(&socket)?;
    // The containing dir is 0700, so this is belt and braces.
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;

    // Send before the daemon exists: the connection sits in the backlog
    // and the bytes in the socket buffer until it accepts. That is why
    // there is no "wait for the daemon to be ready" loop anywhere - and
    // also why there is no reply to read, since nobody is accepting yet.
    let mut stream = UnixStream::connect(&socket)?;
    write_request(&mut stream, &cli.request)?;

    Ok(Started::Listening {
        listener,
        connection: stream,
    })
}

/// Block until the daemon closes the connection, which it does when the
/// window it opened for this request closes. That is what makes `focus`
/// usable as `$EDITOR`.
///
/// Anything still unread is the `ok` for a cold start, where nobody was
/// accepting yet to answer it.
pub fn wait_for_close(mut connection: UnixStream) {
    let mut rest = Vec::new();
    let _ = connection.read_to_end(&mut rest);
}

fn lock(runtime_dir: &Path) -> std::io::Result<File> {
    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path(runtime_dir))?;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(file)
}

fn write_request(stream: &mut UnixStream, request: &Request) -> std::io::Result<()> {
    stream.write_all(&request.encode())?;
    // The shutdown is the frame: it is how the daemon knows the request
    // has ended.
    stream.shutdown(Shutdown::Write)
}

// Just the reply line: the daemon holds the connection open past it,
// until the window closes, so reading to EOF here would block.
fn read_reply(stream: &UnixStream) -> std::io::Result<()> {
    let mut reply = String::new();
    BufReader::new(stream).read_line(&mut reply)?;
    if reply.as_bytes() == REPLY_OK {
        return Ok(());
    }
    Err(std::io::Error::other(reply.trim().to_string()))
}

// ---------------------------------------------------------------------
// Daemon
// ---------------------------------------------------------------------

/// The fd the client dup'd its listener onto, named in the environment so
/// the daemon can find it after the exec.
pub const LISTEN_FD_VAR: &str = "FOCUS_LISTEN_FD";
pub const LISTEN_FD: RawFd = 3;

/// Take back the listening socket the client bound before spawning us.
/// The socket is already bound and listening, and may already have
/// requests queued on it, so there is nothing to wait for.
pub fn listener_from_env() -> std::io::Result<UnixListener> {
    let raw = std::env::var(LISTEN_FD_VAR)
        .map_err(|_| std::io::Error::other(format!("{LISTEN_FD_VAR} is not set")))?;
    let fd: RawFd = raw
        .parse()
        .map_err(|_| std::io::Error::other(format!("{LISTEN_FD_VAR} is not a number: {raw:?}")))?;
    listener_from_fd(fd)
}

/// Take ownership of an inherited listening socket.
pub fn listener_from_fd(fd: RawFd) -> std::io::Result<UnixListener> {
    // Safety: the client dup'd its listener onto this fd before the exec
    // and nothing else in this process has touched it. `local_addr` then
    // rejects anything that is not actually a socket - which is what a
    // hand-typed `--daemon` gets.
    let listener = unsafe { UnixListener::from_raw_fd(fd) };
    listener.local_addr()?;
    Ok(listener)
}

/// A request and the connection it came in on. The daemon holds the
/// connection open until the window it opens closes, so that a client can
/// wait for it by reading to EOF.
pub struct Incoming {
    pub request: Request,
    pub connection: UnixStream,
}

/// Serve requests off `listener` forever, handing each to `sink`.
///
/// Runs on its own thread, so a busy or blocked UI thread never stops the
/// daemon from answering. `sink` is a parameter rather than an
/// `EventLoopProxy` so that tests can hand it a channel.
pub fn serve(listener: UnixListener, sink: impl Fn(Incoming) + Send + 'static) {
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = match stream {
                Ok(stream) => stream,
                // One failed accept says nothing about the next.
                Err(error) => {
                    eprintln!("focus: accept failed: {error}");
                    continue;
                }
            };
            let mut bytes = Vec::new();
            let request = stream
                .read_to_end(&mut bytes)
                .map_err(|error| error.to_string())
                .and_then(|_| Request::decode(&bytes));
            match request {
                Ok(request) => {
                    // Answered here rather than by the UI thread, so a
                    // client learns its request landed without waiting on
                    // the daemon's startup. A client that queued its
                    // request and left is already gone, so a reply that
                    // cannot be delivered is normal.
                    let _ = stream.write_all(REPLY_OK);
                    sink(Incoming {
                        request,
                        connection: stream,
                    });
                }
                Err(message) => {
                    eprintln!("focus: bad request: {message}");
                    let _ = stream.write_all(format!("err {message}\n").as_bytes());
                }
            }
        }
    });
}

/// Start the daemon as a detached process serving `listener`.
///
/// Everything here happens before winit, wayland or GL exist, so there is
/// no fork-after-init hazard.
pub fn spawn(listener: &UnixListener) -> std::io::Result<()> {
    let mut command = Command::new(std::env::current_exe()?);
    command
        .arg("--daemon")
        .env(LISTEN_FD_VAR, LISTEN_FD.to_string())
        // Don't pin the client's cwd - it could be a mount, or get
        // deleted. Nothing in the daemon reads the process cwd.
        .current_dir("/")
        .stdin(Stdio::null());
    match log_file() {
        // Detaching closes the terminal, so without this the daemon's
        // panics would go nowhere.
        Ok(log) => {
            command.stdout(log.try_clone()?).stderr(log);
        }
        Err(error) => {
            eprintln!("focus: no log file ({error}), discarding daemon output");
            command.stdout(Stdio::null()).stderr(Stdio::null());
        }
    }
    let fd = listener.as_raw_fd();
    // Safety: setsid, dup2 and fcntl are all async-signal-safe, which is
    // all a pre_exec closure may use.
    unsafe {
        command.pre_exec(move || {
            // Its own session, so a hangup or a ctrl-c in the launching
            // shell never reaches the daemon.
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            if fd == LISTEN_FD {
                // dup2 onto itself would be a no-op, and would leave
                // CLOEXEC set - so the listener would not survive the
                // exec. Clear it directly instead.
                let flags = libc::fcntl(fd, libc::F_GETFD);
                if flags == -1 || libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
            } else if libc::dup2(fd, LISTEN_FD) == -1 {
                // dup2 clears CLOEXEC on the new fd, which is how the
                // listener gets through the exec.
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    command.spawn()?;
    // Not waited for: it outlives us, and is reparented to init.
    Ok(())
}

fn log_file() -> std::io::Result<File> {
    let dir = match std::env::var_os("XDG_STATE_HOME") {
        Some(dir) => PathBuf::from(dir),
        None => {
            let home = std::env::var_os("HOME")
                .ok_or_else(|| std::io::Error::other("neither XDG_STATE_HOME nor HOME is set"))?;
            PathBuf::from(home).join(".local/state")
        }
    }
    .join("focus");
    std::fs::create_dir_all(&dir)?;
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(dir.join(format!("{APP_ID}.log")))
}
