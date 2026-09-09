use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use focus_core::app::App;
use focus_core::fuzz::MockIO;
use focus_core::input::{Key, NamedKey};
use focus_core::window::WindowId;

mod common;

const FISH_HISTORY_PATH: &str = "/home/.local/share/fish/fish_history";

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

// A scratch app with a few files and a fish history, so every page kind has
// something to list.
fn app_with_files() -> (App, MockIO, WindowId) {
    let (app, mut io, window_id) = common::scratch_app();
    insert_file(&mut io, "/repo/one.rs", "alpha\nbeta\ngamma\n");
    insert_file(&mut io, "/repo/two.rs", "beta\ndelta\n");
    insert_file(&mut io, "/repo/sub/three.rs", "gamma\n");
    insert_file(&mut io, FISH_HISTORY_PATH, "- cmd: make test\n- cmd: ls\n");
    io.git_roots.push(PathBuf::from("/repo"));
    (app, io, window_id)
}

// Everything drawn for a window, as a snapshot string: glyphs, cursors,
// selections and layout. Two windows showing the same thing produce the
// same string.
fn render(app: &mut App, window_id: WindowId) -> String {
    let drawing = common::draw(app, window_id, 40, 12);
    format!("{:#?}", drawing.commands)
}

fn other_window(io: &MockIO, before: &[WindowId]) -> WindowId {
    *io.open_windows
        .iter()
        .find(|window_id| !before.contains(window_id))
        .unwrap()
}

// Press ctrl+n and return the window it opened.
fn duplicate(app: &mut App, io: &mut MockIO, window_id: WindowId) -> WindowId {
    let before = io.open_windows.clone();
    common::control_key(app, io, window_id, Key::Character("n"));
    other_window(io, &before)
}

// Leave `window_id` showing each page kind in turn, from a fresh app.
const OPENERS: &[(&str, fn(&mut App, &mut MockIO, WindowId))] = &[
    ("edit", |_app, _io, _window_id| {}),
    ("open_file", |app, io, window_id| {
        common::control_key(app, io, window_id, Key::Character("o"));
    }),
    ("open_file_from_repo", |app, io, window_id| {
        common::control_key(app, io, window_id, Key::Character("p"));
    }),
    ("open_buffer", |app, io, window_id| {
        common::alt_key(app, io, window_id, Key::Character("p"));
    }),
    ("search_buffer", |app, io, window_id| {
        common::control_key(app, io, window_id, Key::Character("f"));
    }),
    ("search_repo", |app, io, window_id| {
        common::alt_key(app, io, window_id, Key::Character("f"));
    }),
    ("choose_dir", |app, io, window_id| {
        common::control_key(app, io, window_id, Key::Character("m"));
    }),
    ("choose_command", |app, io, window_id| {
        common::control_key(app, io, window_id, Key::Character("m"));
        common::alt_key(app, io, window_id, Key::Named(NamedKey::Enter));
    }),
];

// The runner is deliberately not in OPENERS: its copy starts a fresh run,
// so it is the one page whose copy does not render the same. See
// duplicating_the_runner_runs_the_command_again.
#[test]
fn every_page_kind_duplicates_and_renders_the_same() {
    for (name, open) in OPENERS {
        let (mut app, mut io, window_id) = app_with_files();
        open(&mut app, &mut io, window_id);
        // Let the page fill its lists and previews before copying it.
        common::tick(&mut app, &mut io);

        let copy_window_id = duplicate(&mut app, &mut io, window_id);
        common::tick(&mut app, &mut io);

        assert_eq!(io.open_windows.len(), 2, "{}", name);
        assert_eq!(
            render(&mut app, window_id),
            render(&mut app, copy_window_id),
            "{} did not copy",
            name
        );
        app.assert_invariants();
    }
}

#[test]
fn duplicating_an_edit_page_shares_the_buffer_and_keeps_the_cursor() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "abcdef");
    for _ in 0..2 {
        common::control_key(&mut app, &mut io, window_id, Key::Character("j"));
    }

    let copy_window_id = duplicate(&mut app, &mut io, window_id);
    // The copy starts where the original's cursor was, and types into the
    // same buffer.
    common::char_input(&mut app, &mut io, copy_window_id, 'X');

    assert_eq!(common::text(&app), "abcdXef");
    // The original sees the copy's edit, and typing there lands after it.
    common::char_input(&mut app, &mut io, window_id, 'Y');
    assert_eq!(common::text(&app), "abcdXYef");
    app.assert_invariants();
}

#[test]
fn duplicating_a_picker_copies_its_input_field_without_sharing_it() {
    let (mut app, mut io, window_id) = app_with_files();
    common::control_key(&mut app, &mut io, window_id, Key::Character("p"));
    common::text_input(&mut app, &mut io, window_id, "two");
    common::tick(&mut app, &mut io);
    // scratch_app: editor (0), status bar (1). ctrl+p: preview (2),
    // search (3), list (4). The copy takes the next three.
    const SEARCH: usize = 3;
    const COPY_SEARCH: usize = 6;

    let copy_window_id = duplicate(&mut app, &mut io, window_id);
    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, SEARCH), "two");
    assert_eq!(buffer_text(&app, COPY_SEARCH), "two");

    // The fields have independent copies of the same in-progress undo batch.
    common::control_key(&mut app, &mut io, copy_window_id, Key::Character("z"));
    assert_eq!(buffer_text(&app, SEARCH), "two");
    assert_eq!(buffer_text(&app, COPY_SEARCH), "");
    common::control_key(&mut app, &mut io, copy_window_id, Key::Character("Z"));
    assert_eq!(buffer_text(&app, COPY_SEARCH), "two");

    // Typing in one field leaves the other field alone.
    common::text_input(&mut app, &mut io, copy_window_id, "x");
    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, SEARCH), "two");
    assert_eq!(buffer_text(&app, COPY_SEARCH), "twox");
    app.assert_invariants();
}

// Run `command` from a scratch app, leaving `window_id` on the runner page.
// Buffers: editor (0), status bar (1), dir picker path (2) and list (3),
// command picker preview (4), search (5) and list (6), runner output (7)
// and status (8).
fn runner_app(command: &str) -> (App, MockIO, WindowId) {
    let (mut app, mut io, window_id) = common::scratch_app();
    insert_file(&mut io, FISH_HISTORY_PATH, "");
    common::control_key(&mut app, &mut io, window_id, Key::Character("m"));
    common::alt_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::text_input(&mut app, &mut io, window_id, command);
    common::alt_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    (app, io, window_id)
}

#[test]
fn duplicating_the_runner_runs_the_command_again() {
    const OUTPUT: usize = 7;
    const COPY_OUTPUT: usize = 9;
    let (mut app, mut io, window_id) = runner_app("cargo build");
    io.processes[0].pending_output.extend_from_slice(b"first\n");
    common::tick(&mut app, &mut io);

    let copy_window_id = duplicate(&mut app, &mut io, window_id);
    common::tick(&mut app, &mut io);

    // A second process for the same command in the same dir, and the copy's
    // output starts empty rather than inheriting the first run's.
    assert_eq!(io.processes.len(), 2);
    assert_eq!(io.processes[1].command, io.processes[0].command);
    assert_eq!(io.processes[1].dir, io.processes[0].dir);
    assert_eq!(buffer_text(&app, OUTPUT), "first\n");
    assert_eq!(buffer_text(&app, COPY_OUTPUT), "");

    // Each page drains its own process.
    io.processes[1]
        .pending_output
        .extend_from_slice(b"second\n");
    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, OUTPUT), "first\n");
    assert_eq!(buffer_text(&app, COPY_OUTPUT), "second\n");

    // Closing the copy kills only the copy's process.
    common::control_key(&mut app, &mut io, copy_window_id, Key::Character("q"));
    assert!(io.processes[1].killed);
    assert!(!io.processes[0].killed);
    app.assert_invariants();
}
