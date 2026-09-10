use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use focus_core::app::App;
use focus_core::fuzz::MockIO;
use focus_core::input::{Key, NamedKey};
use focus_core::window::WindowId;

mod common;

// Buffer creation order: open_scratch makes the editor buffer (0) and status
// bar buffer (1); alt+f then makes the preview (2), search (3) and list (4)
// buffers.
const PREVIEW: usize = 2;
const SEARCH: usize = 3;
const LIST: usize = 4;

fn buffer_text(app: &App, n: usize) -> String {
    app.buffers.keys().nth(n).unwrap().text(app).to_string()
}

fn insert_file(io: &mut MockIO, path: &str, text: &str) {
    io.files.insert(
        PathBuf::from(path),
        (
            text.as_bytes().to_vec(),
            SystemTime::UNIX_EPOCH + Duration::from_secs(1),
        ),
    );
}

fn search_repo_app(files: &[(&str, &str)]) -> (App, MockIO, WindowId) {
    let (mut app, mut io, window_id) = common::scratch_app();
    for (path, text) in files {
        insert_file(&mut io, path, text);
    }
    common::alt_key(&mut app, &mut io, window_id, Key::Character("f"));
    (app, io, window_id)
}

#[test]
fn empty_search_has_no_matches() {
    let (mut app, mut io, _window_id) = search_repo_app(&[("/a.txt", "foo")]);

    common::tick(&mut app, &mut io);

    assert_eq!(buffer_text(&app, SEARCH), "");
    assert_eq!(buffer_text(&app, LIST), "");
    app.assert_invariants();
}

#[test]
fn lists_matches_with_path_and_line_prefix() {
    let (mut app, mut io, window_id) = search_repo_app(&[
        ("/b/c.txt", "foo"),
        ("/a.txt", "foo\nbar foo foo"),
        ("/d.txt", "no match"),
    ]);
    common::text_input(&mut app, &mut io, window_id, "foo");

    common::tick(&mut app, &mut io);

    assert_eq!(
        buffer_text(&app, LIST),
        "a.txt:1 foo\na.txt:2 bar foo foo\na.txt:2 bar foo foo\nb/c.txt:1 foo"
    );
    app.assert_invariants();
}

#[test]
fn preview_shows_whole_file_of_selected_match() {
    let (mut app, mut io, window_id) =
        search_repo_app(&[("/a.txt", "foo\nbar foo"), ("/b.txt", "b foo")]);
    common::text_input(&mut app, &mut io, window_id, "foo");

    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, PREVIEW), "foo\nbar foo");

    // Ctrl+k twice selects the match in b.txt.
    common::control_key(&mut app, &mut io, window_id, Key::Character("k"));
    common::control_key(&mut app, &mut io, window_id, Key::Character("k"));
    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, PREVIEW), "b foo");
    app.assert_invariants();
}

#[test]
fn typing_into_the_preview_does_nothing() {
    let (mut app, mut io, window_id) =
        search_repo_app(&[("/a.txt", "foo\nbar foo"), ("/b.txt", "b foo")]);
    common::text_input(&mut app, &mut io, window_id, "foo");
    common::tick(&mut app, &mut io);
    let preview_before = buffer_text(&app, PREVIEW);

    // Focus the generated preview in the top half of the page.
    common::draw(&mut app, window_id, 40, 20);
    let cell_size = app.cell_size();
    let preview_position = [2.0 * cell_size[0] as f32, 2.0 * cell_size[1] as f32];
    common::mouse_enter(&mut app, &mut io, window_id);
    common::mouse_moved(&mut app, &mut io, window_id, preview_position);
    common::char_input(&mut app, &mut io, window_id, 'X');

    assert_eq!(buffer_text(&app, PREVIEW), preview_before);
    app.assert_invariants();
}

#[test]
fn ctrl_enter_opens_selected_match_in_current_window() {
    let (mut app, mut io, window_id) = search_repo_app(&[("/a.txt", "one foo two foo")]);
    common::text_input(&mut app, &mut io, window_id, "foo");
    let buffers_before = app.buffers.keys().count();

    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::char_input(&mut app, &mut io, window_id, 'X');

    // Only the selected match is marked, so only it is replaced.
    assert_eq!(buffer_text(&app, buffers_before), "one X two foo");
    assert_eq!(io.open_windows.len(), 1);
    app.assert_invariants();
}

#[test]
fn ctrl_enter_uses_current_search_before_next_tick() {
    let (mut app, mut io, window_id) =
        search_repo_app(&[("/a.txt", "apple"), ("/b.txt", "banana")]);
    common::text_input(&mut app, &mut io, window_id, "ban");
    let buffers_before = app.buffers.keys().count();

    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::char_input(&mut app, &mut io, window_id, 'X');

    assert_eq!(buffer_text(&app, buffers_before), "Xana");
    app.assert_invariants();
}

#[test]
fn alt_enter_opens_one_window_per_file() {
    let (mut app, mut io, window_id) =
        search_repo_app(&[("/a.txt", "foo one foo"), ("/b.txt", "x foo")]);
    common::text_input(&mut app, &mut io, window_id, "foo");
    let buffers_before = app.buffers.keys().count();

    common::alt_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));

    // The selected match's file replaces this window's page; the other file
    // opens in a new window. Each edit page also creates a status bar buffer.
    assert_eq!(io.open_windows.len(), 2);
    assert_eq!(app.buffers.keys().count(), buffers_before + 4);

    // All matches in the current window's file are marked.
    common::char_input(&mut app, &mut io, window_id, 'X');
    assert_eq!(buffer_text(&app, buffers_before), "X one X");

    // All matches in the new window's file are marked.
    let other_window_id = *io
        .open_windows
        .iter()
        .find(|other| **other != window_id)
        .unwrap();
    common::char_input(&mut app, &mut io, other_window_id, 'Y');
    assert_eq!(buffer_text(&app, buffers_before + 2), "x Y");
    app.assert_invariants();
}

#[test]
fn search_text_is_shared_with_search_buffer() {
    let (mut app, mut io, window_id) = common::file_app(PathBuf::from("/repo/file.txt"), "foo bar");
    common::tick(&mut app, &mut io);

    // Search the buffer for "bar", then open the repo search page.
    common::control_key(&mut app, &mut io, window_id, Key::Character("f"));
    common::text_input(&mut app, &mut io, window_id, "bar");
    let buffers_before = app.buffers.keys().count();
    common::alt_key(&mut app, &mut io, window_id, Key::Character("f"));

    // The repo search page is seeded with the cached text. The search buffer
    // is created after the preview buffer. The search buffer page has no
    // current path, so the search starts from the current dir (/).
    assert_eq!(buffer_text(&app, buffers_before + 1), "bar");
    common::tick(&mut app, &mut io);
    assert_eq!(
        buffer_text(&app, buffers_before + 2),
        "repo/file.txt:1 foo bar"
    );
    app.assert_invariants();
}

#[test]
fn repo_search_text_seeds_search_buffer() {
    let (mut app, mut io, window_id) = search_repo_app(&[("/a.txt", "one foo")]);
    common::text_input(&mut app, &mut io, window_id, "foo");
    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));

    // Ctrl+f on the opened edit page reuses the repo search text.
    let buffers_before = app.buffers.keys().count();
    common::control_key(&mut app, &mut io, window_id, Key::Character("f"));

    assert_eq!(buffer_text(&app, buffers_before), "foo");
    app.assert_invariants();
}

#[test]
fn match_list_stops_at_the_limit() {
    // A short pattern in a big repo matches most lines of most files.
    // Collecting all of them used to cost more memory than the machine has.
    // The limits the page asks for, from page/search_repo.rs.
    let limit = 1000;
    let text = "foo\n".repeat(limit + 100);
    let (mut app, mut io, window_id) =
        search_repo_app(&[("/a.txt", text.as_str()), ("/b.txt", text.as_str())]);
    common::text_input(&mut app, &mut io, window_id, "foo");

    common::tick(&mut app, &mut io);

    let list = buffer_text(&app, LIST);
    let lines: Vec<&str> = list.split('\n').collect();
    // One line per match, plus the truncation note.
    assert_eq!(lines.len(), limit + 1);
    assert_eq!(lines[0], "a.txt:1 foo");
    assert_eq!(lines[limit - 1], format!("a.txt:{} foo", limit));
    assert_eq!(lines[limit], format!("[first {} matches only]", limit));

    // The note line has no match behind it, so it previews nothing and
    // opens nothing.
    for _ in 0..limit {
        common::control_key(&mut app, &mut io, window_id, Key::Character("k"));
    }
    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, PREVIEW), "");
    let buffers_before = app.buffers.keys().count();
    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    assert_eq!(app.buffers.keys().count(), buffers_before);
    app.assert_invariants();
}

#[test]
fn long_match_lines_are_truncated() {
    // One minified file would otherwise hold megabytes on a single line,
    // once per match on that line.
    let line_limit = 500;
    let text = format!("foo{}", "x".repeat(2 * line_limit));
    let (mut app, mut io, window_id) = search_repo_app(&[("/a.txt", text.as_str())]);
    common::text_input(&mut app, &mut io, window_id, "foo");

    common::tick(&mut app, &mut io);

    assert_eq!(
        buffer_text(&app, LIST),
        format!("a.txt:1 foo{}", "x".repeat(line_limit - 3))
    );
    app.assert_invariants();
}
