// Repo search over real files. A short pattern in a big directory matches
// most lines of most files, so the search has to stop somewhere - collecting
// every match used to eat all the memory on the machine.

use std::path::PathBuf;

use bstr::BStr;
use focus::chrome::repo_search;
use focus::search::Search;

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

/// Wait for `Search` to answer, the way the editor does: by asking again
/// every frame. Gives up rather than hanging, so a search that is never
/// answered fails the test it is in.
fn wait_for(search: &Search, dir: &std::path::Path, pattern: &str) -> focus_core::app::RepoSearch {
    for _ in 0..600 {
        if let Some(answer) = search.search(dir, BStr::new(pattern), MATCH_LIMIT, LINE_LIMIT) {
            return answer.unwrap();
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("{pattern:?} was never answered");
}

/// Two search pages are two patterns wanted at once, each asking every
/// frame. Both have to be answered: a worker that kept only the newest
/// request would let them overwrite each other.
#[test]
fn two_patterns_wanted_at_once_are_both_answered() {
    let dir = dir("concurrent");
    std::fs::write(
        dir.join("a.txt"),
        b"apple
",
    )
    .unwrap();
    std::fs::write(
        dir.join("b.txt"),
        b"banana
",
    )
    .unwrap();
    let search = Search::new();

    // Ask for both before either can have been answered, then keep
    // asking for both, as two open pages would.
    assert!(
        search
            .search(&dir, BStr::new("apple"), MATCH_LIMIT, LINE_LIMIT)
            .is_none()
    );
    assert!(
        search
            .search(&dir, BStr::new("banana"), MATCH_LIMIT, LINE_LIMIT)
            .is_none()
    );

    let apple = wait_for(&search, &dir, "apple");
    let banana = wait_for(&search, &dir, "banana");
    assert_eq!(apple.matches.len(), 1);
    assert_eq!(apple.matches[0].line_text, "apple");
    assert_eq!(banana.matches.len(), 1);
    assert_eq!(banana.matches[0].line_text, "banana");
}

/// An answer lasts as long as something is still asking for it, so a
/// page goes on showing its matches rather than flipping back to
/// searching for them.
#[test]
fn an_answer_that_is_still_wanted_is_kept() {
    let dir = dir("kept");
    std::fs::write(
        dir.join("a.txt"),
        b"foo
",
    )
    .unwrap();
    let search = Search::new();

    wait_for(&search, &dir, "foo");
    // Other patterns come and go in the meantime, as typing in another
    // window would produce.
    for n in 0..20 {
        let pattern = format!("miss{n}");
        search.search(&dir, BStr::new(&pattern), MATCH_LIMIT, LINE_LIMIT);
    }
    for _ in 0..50 {
        assert!(
            search
                .search(&dir, BStr::new("foo"), MATCH_LIMIT, LINE_LIMIT)
                .is_some(),
            "an answer still being asked for was forgotten"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}
