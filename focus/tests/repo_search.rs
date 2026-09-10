// Repo search over real files. A short pattern in a big directory matches
// most lines of most files, so the search has to stop somewhere - collecting
// every match used to eat all the memory on the machine.

use std::path::PathBuf;

use bstr::BStr;
use focus::chrome::repo_search;

// Small enough to write the test data by hand. The limits the editor asks
// for live in focus-core's page/search_repo.rs.
const MATCH_LIMIT: usize = 4;
const LINE_LIMIT: usize = 8;

fn dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("focus-search-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::canonicalize(&dir).unwrap()
}

#[test]
fn small_search_is_complete() {
    let dir = dir("small");
    std::fs::write(dir.join("a.txt"), b"foo\nbar foo foo\n").unwrap();
    std::fs::write(dir.join("b.txt"), b"nothing here\n").unwrap();

    let search = repo_search(&dir, BStr::new("foo"), MATCH_LIMIT, LINE_LIMIT).unwrap();

    assert!(!search.truncated);
    assert_eq!(search.matches.len(), 3);
    assert_eq!(search.matches[0].line_text, "foo");
    assert_eq!(search.matches[1].range, 8..11);
}

#[test]
fn search_stops_at_the_match_limit() {
    let dir = dir("limit");
    let text = "foo\n".repeat(MATCH_LIMIT);
    for name in ["a.txt", "b.txt", "c.txt"] {
        std::fs::write(dir.join(name), &text).unwrap();
    }

    let search = repo_search(&dir, BStr::new("foo"), MATCH_LIMIT, LINE_LIMIT).unwrap();

    assert!(search.truncated);
    assert_eq!(search.matches.len(), MATCH_LIMIT);
    // The limit is passed inside the second file the walk reaches, so the
    // third is never searched.
    let mut paths: Vec<&PathBuf> = search
        .matches
        .iter()
        .map(|entry| &entry.relative_path)
        .collect();
    paths.dedup();
    assert_eq!(paths.len(), 1);
}

#[test]
fn long_match_lines_are_truncated() {
    let dir = dir("long-line");
    // One line, one match, and a lot of text after it.
    let mut text = b"foo".to_vec();
    text.extend(std::iter::repeat_n(b'x', 1024));
    std::fs::write(dir.join("minified.js"), &text).unwrap();

    let search = repo_search(&dir, BStr::new("foo"), MATCH_LIMIT, LINE_LIMIT).unwrap();

    assert_eq!(search.matches.len(), 1);
    assert_eq!(search.matches[0].line_text, "fooxxxxx");
}
