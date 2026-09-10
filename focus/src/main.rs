use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

use focus::daemon::{self, Cli, Started};

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    // The one place the process cwd is read: relative paths are resolved
    // here, in the client, because the daemon's own cwd means nothing.
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    let cli = match daemon::parse_args(&args, &cwd) {
        Ok(cli) => cli,
        Err(message) => return fail(&message),
    };

    // Spawned by a client: our listener is already bound, and the request
    // that caused us to exist is already queued on it.
    if cli.daemon {
        return match daemon::listener_from_env() {
            Ok(listener) => {
                focus::chrome::run(listener);
                ExitCode::SUCCESS
            }
            Err(error) => fail(&format!("{error}")),
        };
    }

    match daemon::connect_or_start(&daemon::runtime_dir(), &cli) {
        Ok(Started::NothingToDo) => ExitCode::SUCCESS,
        // A daemon was already running and has the request.
        Ok(Started::Sent(connection)) => wait(&cli, connection),
        Ok(Started::Listening {
            listener,
            connection,
        }) => {
            if cli.foreground {
                // This process is the daemon, so there is nothing to wait
                // for and nobody to read our end of the request.
                drop(connection);
                focus::chrome::run(listener);
                return ExitCode::SUCCESS;
            }
            if let Err(error) = daemon::spawn(&listener) {
                return fail(&format!("could not start the daemon: {error}"));
            }
            // The daemon has its own copy. Holding ours would leave the
            // socket looking alive if the daemon died, so that the next
            // client would connect to a listener nobody serves.
            drop(listener);
            wait(&cli, connection)
        }
        Err(error) => fail(&format!("{error}")),
    }
}

// Waiting is the default, so `focus` works as `$EDITOR` untouched. To get
// the terminal back without closing the window: ctrl-c, which kills this
// process and leaves the window with the daemon.
fn wait(cli: &Cli, connection: std::os::unix::net::UnixStream) -> ExitCode {
    if !cli.no_wait {
        daemon::wait_for_close(connection);
    }
    ExitCode::SUCCESS
}

fn fail(message: &str) -> ExitCode {
    eprintln!("focus: {message}");
    ExitCode::FAILURE
}
