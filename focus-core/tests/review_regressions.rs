//! Reproductions from the repository review. These drive the same App input
//! and tick paths as the desktop backend; no production code is replaced.

mod common;

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use focus_core::input::{Key, NamedKey};

#[test]
fn edit_in_the_load_frame_is_saved() {
    let path = PathBuf::from("/review.txt");
    let (mut app, mut io, window) = common::file_app(path.clone(), "before");
    common::tick_frames(&mut app, &mut io, 1);
    assert_eq!(common::text(&app), "before");

    // Input arrives after the tick, before the next frame starts.
    common::text_input(&mut app, &mut io, window, "X");
    assert_eq!(common::text(&app), "Xbefore");
    common::tick_frames(&mut app, &mut io, 2);
    common::control_key(&mut app, &mut io, window, Key::Character("s"));
    app.assert_invariants();
    assert_eq!(
        io.files[&path].0, b"Xbefore",
        "the edit must remain dirty across frames"
    );
}

#[test]
fn edit_in_the_save_frame_is_saved_again() {
    let path = PathBuf::from("/review.txt");
    let (mut app, mut io, window) = common::file_app(path.clone(), "before");
    common::tick_frames(&mut app, &mut io, 2);
    common::text_input(&mut app, &mut io, window, "X");
    common::control_key(&mut app, &mut io, window, Key::Character("s"));
    assert_eq!(io.files[&path].0, b"Xbefore");

    // Another input event in the same frame as the successful save.
    common::text_input(&mut app, &mut io, window, "Y");
    assert_eq!(common::text(&app), "XYbefore");
    common::tick_frames(&mut app, &mut io, 2);
    common::focus_changed(&mut app, &mut io, window, false);
    app.assert_invariants();
    assert_eq!(
        io.files[&path].0, b"XYbefore",
        "autosave must include the second edit"
    );
}

// Known failure, kept as the reproduction. `matches_for_buffer` copies
// the whole source line into the list once per occurrence, so a line of
// n repeated matches costs O(n * line) bytes: 2 KB of `a` searched for
// `a` builds a 4 MB list. A minified file can be far worse. Fixing it
// means bounding what each entry shows and how many entries are built,
// which changes what the list looks like - so it is a change to make
// deliberately rather than a bug to patch quietly.
#[test]
#[ignore = "known failure: buffer search results are quadratic in a long repetitive line"]
fn buffer_search_bounds_results_for_a_long_repetitive_line() {
    // Small enough to reproduce safely even with the quadratic expansion.
    let (mut app, mut io, window) =
        common::file_app(PathBuf::from("/review.txt"), &"a".repeat(2000));
    common::tick_frames(&mut app, &mut io, 2);
    common::control_key(&mut app, &mut io, window, Key::Character("f"));
    common::text_input(&mut app, &mut io, window, "a");
    common::tick_frames(&mut app, &mut io, 1);

    // Edit page: file + status; search page: input + generated results.
    let list = app.buffers.keys().nth(3).unwrap();
    let results = list.text(&app);
    assert!(
        results.starts_with(b"1 "),
        "the query must actually produce results"
    );
    assert!(
        results.len() <= 1024 * 1024,
        "a 2 KB source produced {} bytes of results; bound snippets/results instead of copying the entire line for each occurrence",
        results.len()
    );
    // A bounded result display must still let the user act on a match.
    common::control_key(&mut app, &mut io, window, Key::Named(NamedKey::Enter));
    common::text_input(&mut app, &mut io, window, "X");
    assert!(common::text(&app).contains('X'));
    app.assert_invariants();
}

// Intended: a closed page keeps its editors and buffers. Ids are dense
// indices into the maps that hold them, and every id already handed out
// stays valid for the life of the process - see `page::teardown`. What
// a page does own is anything outside the process, and teardown is what
// gives that back: the runner kills its shell there.
#[test]
fn closing_a_page_keeps_its_buffers_and_its_shared_file() {
    let source = "needle in this line\n".repeat(100);
    let (mut app, mut io, window) = common::file_app(PathBuf::from("/review.txt"), &source);
    common::tick_frames(&mut app, &mut io, 2);
    common::control_key(&mut app, &mut io, window, Key::Character("f"));
    common::text_input(&mut app, &mut io, window, "needle");
    common::tick_frames(&mut app, &mut io, 1);
    let results = app.buffers.keys().nth(3).unwrap().text(&app).to_vec();
    assert!(results.starts_with(b"1 needle"));
    let buffers = app.buffers.keys().count();

    common::control_key(&mut app, &mut io, window, Key::Character("q"));
    common::tick_frames(&mut app, &mut io, 2);
    assert_eq!(
        common::text(&app),
        source,
        "the shared file must survive closing the search"
    );
    app.assert_invariants();
    assert_eq!(
        app.buffers.keys().count(),
        buffers,
        "closing a page is not meant to renumber anything"
    );
    assert!(
        app.buffers
            .keys()
            .any(|id| id.text(&app) == results.as_slice()),
        "the search's result buffer is meant to outlive its page"
    );
}

#[test]
fn reverting_a_pending_repo_query_restores_results() {
    let (mut app, mut io, window) = common::scratch_app();
    io.files.insert(
        PathBuf::from("/a.txt"),
        (
            b"foo bar".to_vec(),
            SystemTime::UNIX_EPOCH + Duration::from_secs(1),
        ),
    );
    common::alt_key(&mut app, &mut io, window, Key::Character("f"));
    common::text_input(&mut app, &mut io, window, "foo");
    common::tick_frames(&mut app, &mut io, 2);
    let list = app.buffers.keys().nth(4).unwrap();
    assert_eq!(list.text(&app), "a.txt:1 foo bar");

    io.repo_search_pending = true;
    common::text_input(&mut app, &mut io, window, "x");
    common::tick_frames(&mut app, &mut io, 1);
    assert_eq!(list.text(&app), "[searching ...]");
    common::key(&mut app, &mut io, window, Key::Named(NamedKey::Backspace));
    io.repo_search_pending = false;
    common::tick_frames(&mut app, &mut io, 3);
    app.assert_invariants();
    assert_eq!(list.text(&app), "a.txt:1 foo bar");
}

#[test]
fn external_replacement_with_an_older_mtime_reloads() {
    let path = PathBuf::from("/review.txt");
    let (mut app, mut io, _) = common::file_app(path.clone(), "before");
    common::tick_frames(&mut app, &mut io, 1);
    assert_eq!(common::text(&app), "before");

    // A restored backup may preserve a timestamp older than the loaded file.
    io.files.insert(
        path,
        (
            b"restored backup".to_vec(),
            SystemTime::UNIX_EPOCH + Duration::from_millis(500),
        ),
    );
    common::tick_frames(&mut app, &mut io, 2);
    app.assert_invariants();
    assert_eq!(common::text(&app), "restored backup");
}
