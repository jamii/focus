// Writing a file. A save is the one thing the editor does that can lose
// work, so it stages the new contents beside the file and renames them
// over it - see `chrome::file_write`.

use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;

use focus::chrome::file_write;

fn dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("focus-save-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn mode(path: &std::path::Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o7777
}

fn leftovers(dir: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name != "notes.txt")
        .collect();
    names.sort();
    names
}

#[test]
fn a_write_replaces_the_contents_and_keeps_the_mode() {
    let dir = dir("replace");
    let path = dir.join("notes.txt");
    std::fs::write(&path, b"before\n").unwrap();
    // Not a mode the umask would have produced, so that carrying it over
    // is what the assertion sees.
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o606)).unwrap();

    let mtime = file_write(&path, b"after\n", false).unwrap();

    assert_eq!(std::fs::read(&path).unwrap(), b"after\n");
    assert_eq!(mode(&path), 0o606);
    assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), mtime);
    // Nothing staged is left behind.
    assert_eq!(leftovers(&dir), Vec::<String>::new());
}

#[test]
fn a_write_that_creates_the_file_uses_the_usual_mode() {
    let dir = dir("create");
    let path = dir.join("notes.txt");
    // What creating a file here any other way gives it, whatever this
    // machine's umask is.
    let control = dir.join("control.txt");
    std::fs::write(&control, b"control\n").unwrap();

    file_write(&path, b"new\n", true).unwrap();

    assert_eq!(std::fs::read(&path).unwrap(), b"new\n");
    assert_eq!(mode(&path), mode(&control));
    assert_eq!(leftovers(&dir), vec!["control.txt".to_string()]);
}

#[test]
fn a_write_that_may_not_create_reports_a_missing_file() {
    let dir = dir("no-create");
    let path = dir.join("notes.txt");

    let error = file_write(&path, b"new\n", false).unwrap_err();

    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    assert!(!path.exists());
    assert_eq!(leftovers(&dir), Vec::<String>::new());
}

// The failure the staging is for: the write cannot finish, and the file
// that was there is still the file that is there.
#[test]
fn a_write_that_cannot_be_staged_leaves_the_previous_contents() {
    if unsafe { libc::geteuid() } == 0 {
        eprintln!("skipping: root writes through directory permissions");
        return;
    }
    let dir = dir("read-only-dir");
    let path = dir.join("notes.txt");
    std::fs::write(&path, b"before\n").unwrap();
    // Writable file, read-only directory: nothing can be created beside
    // it to stage into.
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).unwrap();

    let error = file_write(&path, b"after\n", false).unwrap_err();

    assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(std::fs::read(&path).unwrap(), b"before\n");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(leftovers(&dir), Vec::<String>::new());
}

// The process ran out of file size before it ran out of contents, which
// is where truncating the original would have destroyed it.
#[test]
fn a_write_that_stops_half_way_leaves_the_previous_contents() {
    let dir = dir("size-limit");
    let path = dir.join("notes.txt");
    std::fs::write(&path, b"before\n").unwrap();

    // SIGXFSZ would take the whole test binary down; ignoring it leaves
    // the EFBIG the write itself reports. Restored below, along with the
    // limit, because both are process-wide.
    let previous = unsafe { libc::signal(libc::SIGXFSZ, libc::SIG_IGN) };
    assert_ne!(previous, libc::SIG_ERR);
    let mut original = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    assert_eq!(
        unsafe { libc::getrlimit(libc::RLIMIT_FSIZE, &mut original) },
        0
    );
    let limited = libc::rlimit {
        rlim_cur: 512,
        rlim_max: original.rlim_max,
    };
    assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_FSIZE, &limited) }, 0);

    let result = file_write(&path, &b"x".repeat(1024), false);

    assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_FSIZE, &original) }, 0);
    unsafe { libc::signal(libc::SIGXFSZ, previous) };

    let error = result.unwrap_err();
    assert_eq!(error.raw_os_error(), Some(libc::EFBIG));
    assert_eq!(std::fs::read(&path).unwrap(), b"before\n");
    assert_eq!(leftovers(&dir), Vec::<String>::new());
}
