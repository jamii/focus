use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use focus_core::app::App;
use focus_core::buffer;
use focus_core::fuzz::MockIO;
use focus_core::input::{Key, NamedKey};
use focus_core::window::WindowId;

mod common;

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

fn add_file_buffer(app: &mut App, io: &mut MockIO, path: &str, text: &str) -> usize {
    insert_file(io, path, text);
    let index = app.buffers.keys().count();
    buffer::from_file(app, io, PathBuf::from(path));
    index
}

fn open_buffer_app(files: &[(&str, &str)]) -> (App, MockIO, WindowId, Vec<usize>, usize, usize) {
    let (mut app, mut io, window_id) = common::scratch_app();
    let mut file_indices = Vec::new();
    for (path, text) in files {
        file_indices.push(add_file_buffer(&mut app, &mut io, path, text));
    }
    let page_buffer_start = app.buffers.keys().count();
    common::alt_key(&mut app, &mut io, window_id, Key::Character("p"));
    let search = page_buffer_start + 1;
    let list = page_buffer_start + 2;
    (app, io, window_id, file_indices, search, list)
}

#[test]
fn lists_file_buffers_sorted_by_path() {
    let (mut app, mut io, _window_id, _files, _search, list) = open_buffer_app(&[
        ("/banana.txt", "banana contents"),
        ("/apple.txt", "apple contents"),
        ("/dir/inner.txt", "inner contents"),
    ]);
    common::tick(&mut app, &mut io);

    assert_eq!(
        buffer_text(&app, list),
        "/apple.txt\n/banana.txt\n/dir/inner.txt"
    );
    app.assert_invariants();
}

#[test]
fn includes_current_buffer() {
    let (mut app, mut io, window_id) = common::file_app(PathBuf::from("/current.txt"), "current");
    add_file_buffer(&mut app, &mut io, "/other.txt", "other");
    let page_buffer_start = app.buffers.keys().count();

    common::alt_key(&mut app, &mut io, window_id, Key::Character("p"));
    common::tick(&mut app, &mut io);

    assert_eq!(
        buffer_text(&app, page_buffer_start + 2),
        "/current.txt\n/other.txt"
    );
    app.assert_invariants();
}

#[test]
fn scratch_buffers_are_not_listed() {
    let (mut app, mut io, window_id) = common::scratch_app();
    let page_buffer_start = app.buffers.keys().count();

    common::alt_key(&mut app, &mut io, window_id, Key::Character("p"));
    common::tick(&mut app, &mut io);

    assert_eq!(buffer_text(&app, page_buffer_start + 2), "");
    app.assert_invariants();
}

#[test]
fn fuzzy_match_uses_absolute_path() {
    let (mut app, mut io, window_id, _files, search, list) = open_buffer_app(&[
        ("/main.rs", "root main"),
        ("/src/main.rs", "src main"),
        ("/src/lib.rs", "lib"),
    ]);
    common::text_input(&mut app, &mut io, window_id, "sm");
    assert_eq!(buffer_text(&app, search), "sm");

    common::tick(&mut app, &mut io);

    assert_eq!(buffer_text(&app, list), "/src/main.rs");
    app.assert_invariants();
}

#[test]
fn preview_uses_selected_buffer_directly() {
    let (mut app, mut io, window_id, files, _search, _list) = open_buffer_app(&[
        ("/apple.txt", "apple contents"),
        ("/banana.txt", "banana contents"),
    ]);
    common::tick(&mut app, &mut io);

    common::draw(&mut app, window_id, 40, 20);
    let cell_size = app.cell_size();
    let preview_position = [2.0 * cell_size[0] as f32, 2.0 * cell_size[1] as f32];
    common::mouse_moved(&mut app, &mut io, window_id, preview_position);
    common::char_input(&mut app, &mut io, window_id, 'X');

    assert_eq!(buffer_text(&app, files[0]), "Xapple contents");
    app.assert_invariants();
}

#[test]
fn ctrl_ik_move_the_selection() {
    let (mut app, mut io, window_id, files, _search, _list) = open_buffer_app(&[
        ("/apple.txt", "apple contents"),
        ("/banana.txt", "banana contents"),
    ]);
    common::tick(&mut app, &mut io);

    common::control_key(&mut app, &mut io, window_id, Key::Character("k"));
    common::tick(&mut app, &mut io);

    common::draw(&mut app, window_id, 40, 20);
    let cell_size = app.cell_size();
    let preview_position = [2.0 * cell_size[0] as f32, 2.0 * cell_size[1] as f32];
    common::mouse_moved(&mut app, &mut io, window_id, preview_position);
    common::char_input(&mut app, &mut io, window_id, 'X');

    assert_eq!(buffer_text(&app, files[1]), "Xbanana contents");
    app.assert_invariants();
}

#[test]
fn ctrl_enter_opens_selected_buffer() {
    let (mut app, mut io, window_id, _files, _search, _list) = open_buffer_app(&[
        ("/apple.txt", "apple contents"),
        ("/banana.txt", "banana contents"),
    ]);
    common::tick(&mut app, &mut io);
    common::control_key(&mut app, &mut io, window_id, Key::Character("k"));
    common::tick(&mut app, &mut io);
    let buffers_before = app.buffers.keys().count();

    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::tick(&mut app, &mut io);

    // Only a status bar buffer is created; the file buffer is reused.
    assert_eq!(app.buffers.keys().count(), buffers_before + 1);
    assert_eq!(buffer_text(&app, 3), "banana contents");
    app.assert_invariants();
}

#[test]
fn ctrl_shift_enter_opens_selected_buffer_in_new_window() {
    let (mut app, mut io, window_id, files, search, _list) =
        open_buffer_app(&[("/apple.txt", "apple contents")]);
    common::tick(&mut app, &mut io);

    common::control_shift_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    let window_id_new = *io.open_windows.last().unwrap();
    common::char_input(&mut app, &mut io, window_id_new, 'X');
    common::char_input(&mut app, &mut io, window_id, 'z');

    assert_eq!(io.open_windows.len(), 2);
    assert_eq!(buffer_text(&app, files[0]), "Xapple contents");
    assert_eq!(buffer_text(&app, search), "z");
    app.assert_invariants();
}

#[test]
fn ctrl_enter_uses_current_search_before_next_tick() {
    let (mut app, mut io, window_id, _files, _search, _list) = open_buffer_app(&[
        ("/apple.txt", "apple contents"),
        ("/banana.txt", "banana contents"),
    ]);
    common::text_input(&mut app, &mut io, window_id, "ban");
    let buffers_before = app.buffers.keys().count();

    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::tick(&mut app, &mut io);

    assert_eq!(app.buffers.keys().count(), buffers_before + 1);
    assert_eq!(buffer_text(&app, 3), "banana contents");
    app.assert_invariants();
}

#[test]
fn list_updates_when_buffers_change() {
    let (mut app, mut io, _window_id, _files, _search, list) =
        open_buffer_app(&[("/old.txt", "old")]);
    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, list), "/old.txt");

    add_file_buffer(&mut app, &mut io, "/new.txt", "new");
    common::tick(&mut app, &mut io);

    assert_eq!(buffer_text(&app, list), "/new.txt\n/old.txt");
    app.assert_invariants();
}

#[test]
fn alt_enter_does_nothing() {
    let (mut app, mut io, window_id, _files, _search, _list) =
        open_buffer_app(&[("/file.txt", "contents")]);
    common::tick(&mut app, &mut io);
    let buffers_before = app.buffers.keys().count();

    common::alt_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::tick(&mut app, &mut io);

    assert_eq!(app.buffers.keys().count(), buffers_before);
    app.assert_invariants();
}
