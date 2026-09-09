// Path canonicalization. Buffers are keyed by path, so two spellings of
// one file must arrive spelled the same way - otherwise the file gets two
// buffers, and whichever saves last wins.

use std::path::{Path, PathBuf};

use focus::chrome::canonical_path;

fn dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("focus-paths-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // The temp dir itself can be a symlink (/tmp -> /private/tmp and the
    // like), so compare against its resolved form.
    std::fs::canonicalize(&dir).unwrap()
}

#[test]
fn dot_dot_is_resolved() {
    let dir = dir("dotdot");
    std::fs::create_dir(dir.join("sub")).unwrap();
    std::fs::write(dir.join("notes.txt"), b"x").unwrap();

    assert_eq!(
        canonical_path(&dir.join("sub/../notes.txt")),
        dir.join("notes.txt")
    );
    assert_eq!(
        canonical_path(&dir.join("./notes.txt")),
        dir.join("notes.txt")
    );
}

#[test]
fn symlinks_are_resolved() {
    let dir = dir("symlink");
    std::fs::create_dir(dir.join("real")).unwrap();
    std::fs::write(dir.join("real/notes.txt"), b"x").unwrap();
    std::os::unix::fs::symlink(dir.join("real"), dir.join("link")).unwrap();

    // The whole point of doing this in the filesystem rather than
    // textually: `link/../notes.txt` is NOT `notes.txt`.
    assert_eq!(
        canonical_path(&dir.join("link/notes.txt")),
        dir.join("real/notes.txt")
    );
    assert_eq!(
        canonical_path(&dir.join("link/../real/notes.txt")),
        dir.join("real/notes.txt")
    );
}

// `focus new-notes.txt` is how you make a file, so a path that does not
// exist yet still has to normalize.
#[test]
fn a_file_that_does_not_exist_yet_resolves_through_its_dir() {
    let dir = dir("missing-file");
    std::fs::create_dir(dir.join("sub")).unwrap();

    assert_eq!(
        canonical_path(&dir.join("sub/../new.txt")),
        dir.join("new.txt")
    );
}

// Nothing to resolve against, so it is left alone rather than guessed at.
#[test]
fn a_path_under_a_missing_dir_is_left_alone() {
    let dir = dir("missing-dir");
    let path = dir.join("nope/new.txt");

    assert_eq!(canonical_path(&path), path);
}

#[test]
fn the_root_is_left_alone() {
    assert_eq!(canonical_path(Path::new("/")), PathBuf::from("/"));
}
