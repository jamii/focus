// End-to-end test of the daemon, under a real (headless) compositor.
//
// The client tests in daemon.rs stop at the exec. Everything past it -
// setsid, the stdio redirection, the listener surviving the exec, winit
// delivering the request, a window actually appearing - can only be
// tested by running the thing. So this brings up sway with its headless
// backend and drives the real binary against it, asserting through
// `swaymsg -t get_tree`.
//
// Skipped, not failed, when sway is not on PATH.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const FOCUS: &str = env!("CARGO_BIN_EXE_focus");

// Long enough for a debug-build daemon to start and map a window on a
// loaded machine; the polling means a fast machine never waits this long.
const DEADLINE: Duration = Duration::from_secs(20);
const POLL: Duration = Duration::from_millis(50);

fn sway_available() -> bool {
    Command::new("sway")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

#[test]
fn daemon_lifecycle() {
    if !sway_available() {
        eprintln!("skipping: sway is not on PATH");
        return;
    }
    let sway = Sway::start_named("lifecycle");

    // A cold start: no daemon, so the client starts one, and its request
    // is already queued on the socket the client bound.
    sway.focus(&["--no-wait", "a.txt"]);
    assert!(
        sway.wait_for_windows(1),
        "no window appeared; daemon log:\n{}",
        sway.log()
    );
    let pid = sway
        .daemon_pid()
        .expect("exactly one daemon should be running");

    // Its own session, so a hangup in the launching shell cannot reach
    // it. Nothing else catches a missing setsid.
    assert_eq!(session_id(pid), pid, "the daemon is not a session leader");
    // Not holding the client's cwd open.
    assert_eq!(
        std::fs::read_link(format!("/proc/{pid}/cwd")).unwrap(),
        Path::new("/")
    );

    // A second client reuses the daemon rather than starting another.
    sway.focus(&["--no-wait", "b.txt"]);
    assert!(sway.wait_for_windows(2));
    assert_eq!(sway.daemon_pid(), Some(pid));

    // The requirement: closing the last window leaves the daemon up.
    sway.swaymsg(&["[app_id=\"focus-debug\"] kill"]);
    assert!(sway.wait_for_windows(0));
    assert_eq!(sway.daemon_pid(), Some(pid));

    // An idle daemon with no windows must not spin at the frame rate.
    let idle = cpu_ticks(pid);
    std::thread::sleep(Duration::from_secs(2));
    assert!(
        cpu_ticks(pid) - idle <= 2,
        "an idle daemon with no windows burned {} ticks in 2s",
        cpu_ticks(pid) - idle
    );

    // And it still answers.
    sway.focus(&["--no-wait", "--launcher"]);
    assert!(sway.wait_for_windows(1));
    assert_eq!(sway.daemon_pid(), Some(pid));

    // --replace swaps in a new daemon. It does not wait for the old one
    // to die, so both exist for a moment - what matters is that it
    // settles on exactly one, and that it is not the old one.
    sway.focus(&["--no-wait", "--replace", "a.txt"]);
    assert!(
        sway.wait_for(|| sway.daemon_pid().is_some_and(|new| new != pid)),
        "--replace left {:?}, expected one daemon other than {pid}",
        sway.daemon_pids()
    );
    assert!(sway.wait_for_windows(1));

    // --quit is the only thing that takes it down.
    sway.focus(&["--quit"]);
    assert!(sway.wait_for(|| sway.daemon_pids().is_empty()));
    assert!(sway.wait_for_windows(0));

    // --quit with nothing running is a no-op, not a new daemon.
    sway.focus(&["--quit"]);
    assert!(sway.daemon_pids().is_empty());

    // Bare `focus` - the commonest invocation - opens a scratch window.
    sway.focus(&["--no-wait"]);
    assert!(sway.wait_for_windows(1));
    assert!(sway.daemon_pid().is_some());
    sway.focus(&["--quit"]);
    assert!(sway.wait_for(|| sway.daemon_pids().is_empty()));
}

// The dev loop: `--foreground` makes *this* process the daemon, so it can
// be run under a debugger or profiler and killed to restart.
#[test]
fn foreground_runs_the_daemon_in_the_client_process() {
    if !sway_available() {
        eprintln!("skipping: sway is not on PATH");
        return;
    }
    let sway = Sway::start_named("foreground");

    let mut foreground = Command::new(FOCUS)
        .args(["--foreground", "a.txt"])
        .current_dir(&sway.dir)
        .envs(sway.env())
        .spawn()
        .unwrap();

    assert!(sway.wait_for_windows(1));
    // No detached daemon: the window belongs to the process we started.
    assert!(sway.daemon_pids().is_empty());
    assert!(
        foreground.try_wait().unwrap().is_none(),
        "it should still be running"
    );

    // And --replace evicts it, exactly as it evicts a detached one.
    sway.focus(&["--replace", "--quit"]);
    assert!(
        sway.wait_for(|| foreground.try_wait().unwrap().is_some()),
        "--replace did not stop the foreground daemon"
    );
}

// Waiting is the default, so that `focus` works as $EDITOR untouched:
// git runs it on COMMIT_EDITMSG and reads the file back when it exits.
#[test]
fn a_client_waits_until_its_window_closes() {
    if !sway_available() {
        eprintln!("skipping: sway is not on PATH");
        return;
    }
    let sway = Sway::start_named("wait");

    let mut client = Command::new(FOCUS)
        .arg("a.txt")
        .current_dir(&sway.dir)
        .envs(sway.env())
        .spawn()
        .unwrap();
    assert!(sway.wait_for_windows(1));

    // Still blocked, with the window open - this is where git would be
    // sitting while the commit message is written.
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        client.try_wait().unwrap().is_none(),
        "the client exited while its window was still open"
    );

    sway.swaymsg(&["[app_id=\"focus-debug\"] kill"]);

    assert!(
        sway.wait_for(|| client.try_wait().unwrap().is_some()),
        "the client did not return when its window closed"
    );
    // Closing that window did not take the daemon with it.
    assert!(sway.daemon_pid().is_some());

    // --no-wait is the opt-out, and returns with the window still open.
    sway.focus(&["--no-wait", "b.txt"]);
    assert!(sway.wait_for_windows(1));
}

// The binary has to run outside the nix-shell that built it. winit and
// glutin `dlopen` libwayland-client, libxkbcommon and libEGL, and inside
// the shell those resolve only through LD_LIBRARY_PATH - which a normal
// desktop session does not have. shell.nix bakes the same paths into the
// RUNPATH so that `dlopen` finds them anyway; this is what notices when
// it stops doing that.
#[test]
fn the_daemon_runs_without_ld_library_path() {
    if !sway_available() {
        eprintln!("skipping: sway is not on PATH");
        return;
    }
    let sway = Sway::start_named("no-ld-library-path");

    // The daemon inherits the client's environment, so clearing it here
    // clears it for the process that actually opens the window.
    let status = Command::new(FOCUS)
        .args(["--no-wait", "a.txt"])
        .current_dir(&sway.dir)
        .envs(sway.env())
        .env_remove("LD_LIBRARY_PATH")
        .status()
        .unwrap();
    assert!(status.success(), "focus failed: {status}");
    assert!(
        sway.wait_for_windows(1),
        "no window without LD_LIBRARY_PATH - is the RUNPATH still baked in?\n{}",
        sway.log()
    );
}

// A headless sway, its runtime dir, and everything spawned into it. The
// Drop impl runs even when an assertion fails, so a panicking test does
// not leave a compositor and a daemon behind.
struct Sway {
    dir: PathBuf,
    sway: Child,
    swaysock: PathBuf,
}

impl Sway {
    fn start_named(name: &str) -> Sway {
        // Short, because sway's IPC socket path has to fit in a
        // sockaddr_un - a temp dir under the usual test paths does not.
        let dir = PathBuf::from(format!("/tmp/focus-e2e-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("sway.config"),
            // xwayland would fight over the X11 socket with anything else
            // on the machine, and nothing here needs it.
            "xwayland disable\noutput HEADLESS-1 resolution 1280x720\n",
        )
        .unwrap();
        std::fs::write(dir.join("a.txt"), b"hello\n").unwrap();
        std::fs::write(dir.join("b.txt"), b"world\n").unwrap();

        let log = std::fs::File::create(dir.join("sway.log")).unwrap();
        let sway = Command::new("sway")
            .arg("-c")
            .arg(dir.join("sway.config"))
            .env("XDG_RUNTIME_DIR", &dir)
            .env("WLR_BACKENDS", "headless")
            .env("WLR_LIBINPUT_NO_DEVICES", "1")
            .env("WLR_RENDERER", "pixman")
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .unwrap();

        let mut sway = Sway {
            dir,
            sway,
            swaysock: PathBuf::new(),
        };
        assert!(
            sway.wait_for(|| sway.dir.join("wayland-1").exists() && sway.find_swaysock().is_some()),
            "sway did not come up:\n{}",
            std::fs::read_to_string(sway.dir.join("sway.log")).unwrap_or_default()
        );
        sway.swaysock = sway.find_swaysock().unwrap();
        sway
    }

    fn find_swaysock(&self) -> Option<PathBuf> {
        std::fs::read_dir(&self.dir)
            .ok()?
            .flatten()
            .find_map(|entry| {
                let name = entry.file_name().into_string().ok()?;
                (name.starts_with("sway-ipc.") && name.ends_with(".sock")).then(|| entry.path())
            })
    }

    fn env(&self) -> [(&str, PathBuf); 4] {
        [
            ("XDG_RUNTIME_DIR", self.dir.clone()),
            // Keeps the daemon's log inside the test's dir.
            ("XDG_STATE_HOME", self.dir.join("state")),
            ("WAYLAND_DISPLAY", PathBuf::from("wayland-1")),
            ("SWAYSOCK", self.swaysock.clone()),
        ]
    }

    fn focus(&self, args: &[&str]) {
        let status = Command::new(FOCUS)
            .args(args)
            .current_dir(&self.dir)
            .envs(self.env())
            .status()
            .unwrap();
        assert!(status.success(), "focus {args:?} failed: {status}");
    }

    fn swaymsg(&self, args: &[&str]) -> String {
        let output = Command::new("swaymsg")
            .args(args)
            .envs(self.env())
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    fn window_count(&self) -> usize {
        let tree = self.swaymsg(&["-t", "get_tree"]);
        tree.matches(&format!("\"app_id\": \"{}\"", focus::APP_ID))
            .count()
    }

    /// The daemons serving *this test's* runtime dir - found by their
    /// environment, so one left over from another run, or the
    /// developer's own editor, is never mistaken for them.
    ///
    /// Plural because `--replace` deliberately does not wait for the old
    /// daemon to die: for a moment there are two.
    fn daemon_pids(&self) -> Vec<u32> {
        let mut pids = Vec::new();
        let Ok(entries) = std::fs::read_dir("/proc") else {
            return pids;
        };
        for entry in entries.flatten() {
            let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
                continue;
            };
            let Ok(cmdline) = std::fs::read(format!("/proc/{pid}/cmdline")) else {
                continue;
            };
            let args: Vec<&[u8]> = cmdline.split(|byte| *byte == 0).collect();
            if args.first() != Some(&FOCUS.as_bytes()) || !args.contains(&b"--daemon".as_slice()) {
                continue;
            }
            if environ(pid).get("XDG_RUNTIME_DIR").map(PathBuf::from) == Some(self.dir.clone()) {
                pids.push(pid);
            }
        }
        pids
    }

    fn daemon_pid(&self) -> Option<u32> {
        match self.daemon_pids().as_slice() {
            [pid] => Some(*pid),
            _ => None,
        }
    }

    fn wait_for(&self, mut check: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + DEADLINE;
        while Instant::now() < deadline {
            if check() {
                return true;
            }
            std::thread::sleep(POLL);
        }
        check()
    }

    fn wait_for_windows(&self, count: usize) -> bool {
        self.wait_for(|| self.window_count() == count)
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.dir.join(format!("state/focus/{}.log", focus::APP_ID)))
            .unwrap_or_default()
    }
}

impl Drop for Sway {
    fn drop(&mut self) {
        for pid in self.daemon_pids() {
            unsafe { libc::kill(pid as i32, libc::SIGKILL) };
        }
        let _ = self.sway.kill();
        let _ = self.sway.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn environ(pid: u32) -> HashMap<String, String> {
    let Ok(bytes) = std::fs::read(format!("/proc/{pid}/environ")) else {
        return HashMap::new();
    };
    bytes
        .split(|byte| *byte == 0)
        .filter_map(|entry| {
            let entry = String::from_utf8_lossy(entry);
            let (name, value) = entry.split_once('=')?;
            Some((name.to_string(), value.to_string()))
        })
        .collect()
}

// /proc/<pid>/stat, with the fields counted from the one after the comm
// field - which can itself contain spaces, so start after its closing
// paren. Session id is field 6, utime and stime are 14 and 15.
fn stat_field(pid: u32, field: usize) -> u64 {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
    let after_comm = &stat[stat.rfind(')').unwrap() + 1..];
    let fields: Vec<&str> = after_comm.split_whitespace().collect();
    fields[field - 3].parse().unwrap()
}

fn session_id(pid: u32) -> u32 {
    stat_field(pid, 6) as u32
}

fn cpu_ticks(pid: u32) -> u64 {
    stat_field(pid, 14) + stat_field(pid, 15)
}
