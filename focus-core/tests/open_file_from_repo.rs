use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use focus_core::app::App;
use focus_core::fuzz::MockIO;
use focus_core::input::{Key, NamedKey};
use focus_core::window::WindowId;

mod common;

// Buffer creation order: open_scratch/open_edit makes the editor buffer (0)
// and status bar buffer (1); ctrl+p then makes the preview (2), search (3) and
// list (4) buffers.
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

fn open_repo_file_app(files: &[(&str, &str)]) -> (App, MockIO, WindowId) {
    let (mut app, mut io, window_id) = common::scratch_app();
    for (path, text) in files {
        insert_file(&mut io, path, text);
    }
    common::control_key(&mut app, &mut io, window_id, Key::Character("p"));
    (app, io, window_id)
}

#[test]
fn lists_repo_files_sorted_by_relative_path() {
    let (mut app, mut io, _window_id) = open_repo_file_app(&[
        ("/banana.txt", ""),
        ("/apple.txt", ""),
        ("/dir/inner.txt", ""),
    ]);
    common::tick(&mut app, &mut io);
    assert_eq!(
        buffer_text(&app, LIST),
        "apple.txt\nbanana.txt\ndir/inner.txt"
    );
    app.assert_invariants();
}

#[test]
fn starts_from_git_root_when_current_file_is_in_repo() {
    let (mut app, mut io, window_id) = common::file_app(PathBuf::from("/repo/src/main.rs"), "main");
    insert_file(&mut io, "/repo/readme.md", "readme");
    insert_file(&mut io, "/elsewhere.txt", "elsewhere");
    io.git_roots.push(PathBuf::from("/repo"));

    common::control_key(&mut app, &mut io, window_id, Key::Character("p"));
    common::tick(&mut app, &mut io);

    assert_eq!(buffer_text(&app, LIST), "readme.md\nsrc/main.rs");
    app.assert_invariants();
}

#[test]
fn starts_from_home_dir_without_git_root() {
    let (mut app, mut io, window_id) = common::file_app(PathBuf::from("/repo/src/main.rs"), "main");
    insert_file(&mut io, "/repo/src/lib.rs", "lib");
    insert_file(&mut io, "/repo/readme.md", "readme");

    common::control_key(&mut app, &mut io, window_id, Key::Character("p"));
    common::tick(&mut app, &mut io);

    assert_eq!(buffer_text(&app, LIST), "lib.rs\nmain.rs");
    app.assert_invariants();
}

#[test]
fn fuzzy_match_uses_full_relative_path() {
    let (mut app, mut io, window_id) = open_repo_file_app(&[
        ("/main.rs", "root main"),
        ("/src/main.rs", "src main"),
        ("/src/lib.rs", "lib"),
    ]);
    common::text_input(&mut app, &mut io, window_id, "sm");
    assert_eq!(buffer_text(&app, SEARCH), "sm");

    common::tick(&mut app, &mut io);

    assert_eq!(buffer_text(&app, LIST), "src/main.rs");
    assert_eq!(buffer_text(&app, PREVIEW), "src main");
    app.assert_invariants();
}

#[test]
fn file_list_is_captured_once() {
    let (mut app, mut io, window_id) = open_repo_file_app(&[("/old.txt", "old")]);
    insert_file(&mut io, "/new.txt", "new");

    common::text_input(&mut app, &mut io, window_id, "new");
    common::tick(&mut app, &mut io);

    assert_eq!(buffer_text(&app, LIST), "");
    assert_eq!(buffer_text(&app, PREVIEW), "");
    app.assert_invariants();
}

#[test]
fn ctrl_ik_move_the_selection() {
    let (mut app, mut io, window_id) = open_repo_file_app(&[
        ("/apple.txt", "apple contents"),
        ("/banana.txt", "banana contents"),
    ]);
    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, PREVIEW), "apple contents");

    common::control_key(&mut app, &mut io, window_id, Key::Character("k"));
    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, PREVIEW), "banana contents");

    common::control_key(&mut app, &mut io, window_id, Key::Character("i"));
    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, PREVIEW), "apple contents");
    app.assert_invariants();
}

#[test]
fn ctrl_enter_opens_selected_file() {
    let (mut app, mut io, window_id) = open_repo_file_app(&[
        ("/apple.txt", "apple contents"),
        ("/banana.txt", "banana contents"),
    ]);
    common::tick(&mut app, &mut io);
    common::control_key(&mut app, &mut io, window_id, Key::Character("k"));
    common::tick(&mut app, &mut io);
    let buffers_before = app.buffers.keys().count();

    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::tick(&mut app, &mut io);
    common::tick(&mut app, &mut io);

    assert_eq!(app.buffers.keys().count(), buffers_before + 2);
    assert_eq!(buffer_text(&app, buffers_before), "banana contents");
    app.assert_invariants();
}

#[test]
fn ctrl_enter_uses_current_search_before_next_tick() {
    let (mut app, mut io, window_id) = open_repo_file_app(&[
        ("/apple.txt", "apple contents"),
        ("/banana.txt", "banana contents"),
    ]);
    common::text_input(&mut app, &mut io, window_id, "ban");
    let buffers_before = app.buffers.keys().count();

    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::tick(&mut app, &mut io);
    common::tick(&mut app, &mut io);

    assert_eq!(buffer_text(&app, buffers_before), "banana contents");
    app.assert_invariants();
}

#[test]
fn alt_enter_does_nothing() {
    let (mut app, mut io, window_id) = open_repo_file_app(&[("/file.txt", "contents")]);
    common::tick(&mut app, &mut io);
    let buffers_before = app.buffers.keys().count();

    common::alt_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::tick(&mut app, &mut io);

    assert_eq!(app.buffers.keys().count(), buffers_before);
    assert_eq!(buffer_text(&app, SEARCH), "");
    app.assert_invariants();
}
