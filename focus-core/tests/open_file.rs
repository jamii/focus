use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use focus_core::app::App;
use focus_core::drawing::{DrawCommand, Drawing};
use focus_core::fuzz::MockIO;
use focus_core::input::{Key, NamedKey};
use focus_core::style::HIGHLIGHT_COLOR;
use focus_core::window::WindowId;

mod common;

// Buffer creation order: open_scratch makes the editor buffer (0) and status
// bar buffer (1); ctrl+o then makes the preview (2), path (3) and list (4)
// buffers.
const PREVIEW: usize = 2;
const PATH: usize = 3;
const LIST: usize = 4;

fn buffer_text(app: &App, n: usize) -> String {
    app.buffers.keys().nth(n).unwrap().text(app).to_string()
}

fn open_file_app(files: &[(&str, &str)]) -> (App, MockIO, WindowId) {
    let (mut app, mut io, window_id) = common::scratch_app();
    for (path, text) in files {
        io.files.insert(
            PathBuf::from(path),
            (
                text.as_bytes().to_vec(),
                SystemTime::UNIX_EPOCH + Duration::from_secs(1),
            ),
        );
    }
    common::control_key(&mut app, &mut io, window_id, Key::Character("o"));
    (app, io, window_id)
}

#[test]
fn path_starts_with_home_dir() {
    let (app, _io, _window_id) = open_file_app(&[]);
    assert_eq!(buffer_text(&app, PATH), "/");
}

#[test]
fn path_starts_with_current_file_dir() {
    let (mut app, mut io, window_id) = common::file_app(PathBuf::from("/dir/file.txt"), "");
    common::control_key(&mut app, &mut io, window_id, Key::Character("o"));
    assert_eq!(buffer_text(&app, PATH), "/dir/");
    app.assert_invariants();
}

#[test]
fn lists_dir_sorted_by_name() {
    let (mut app, mut io, _window_id) = open_file_app(&[
        ("/banana.txt", ""),
        ("/apple.txt", ""),
        ("/dir/inner.txt", ""),
    ]);
    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, LIST), "apple.txt\nbanana.txt\ndir/");
    app.assert_invariants();
}

#[test]
fn fuzzy_match_filters_and_ranks() {
    let (mut app, mut io, window_id) =
        open_file_app(&[("/axbxc.txt", ""), ("/ABC.txt", ""), ("/nope.md", "")]);
    common::text_input(&mut app, &mut io, window_id, "abc");
    assert_eq!(buffer_text(&app, PATH), "/abc");
    common::tick(&mut app, &mut io);
    // Case-insensitive; the tighter match ranks first; non-matches drop out.
    assert_eq!(buffer_text(&app, LIST), "ABC.txt\naxbxc.txt");
    app.assert_invariants();
}

#[test]
fn missing_dir_shows_error_in_list() {
    let (mut app, mut io, window_id) = open_file_app(&[("/file.txt", "contents")]);
    common::text_input(&mut app, &mut io, window_id, "nowhere/");
    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, LIST), "/nowhere/: entity not found");
    assert_eq!(buffer_text(&app, PREVIEW), "");
    app.assert_invariants();
}

#[test]
fn preview_shows_selected_file() {
    let (mut app, mut io, _window_id) = open_file_app(&[
        ("/apple.txt", "apple contents"),
        ("/banana.txt", "banana contents"),
    ]);
    common::tick(&mut app, &mut io);
    // The list cursor starts on the first line.
    assert_eq!(buffer_text(&app, PREVIEW), "apple contents");
    app.assert_invariants();
}

#[test]
fn preview_shows_nothing_for_dirs() {
    let (mut app, mut io, _window_id) = open_file_app(&[("/dir/inner.txt", "inner contents")]);
    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, LIST), "dir/");
    assert_eq!(buffer_text(&app, PREVIEW), "");
    app.assert_invariants();
}

#[test]
fn preview_is_truncated() {
    let contents = "x".repeat(20_000);
    let (mut app, mut io, _window_id) = open_file_app(&[("/big.txt", &contents)]);
    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, PREVIEW).len(), 10 * 1024);
    app.assert_invariants();
}

#[test]
fn selection_follows_list_cursor() {
    let (mut app, mut io, window_id) = open_file_app(&[
        ("/apple.txt", "apple contents"),
        ("/banana.txt", "banana contents"),
    ]);
    common::tick(&mut app, &mut io);
    // Draw to lay out the editors, then focus the list editor (the bottom
    // half is path row + list) and move its cursor down a line.
    common::draw(&mut app, window_id, 40, 20);
    let cell_size = app.cell_size();
    let list_position = [
        2.0 * cell_size[0] as f32,
        16.0 * cell_size[1] as f32, // in the list, well below the path row
    ];
    common::mouse_enter(&mut app, &mut io, window_id);
    common::mouse_moved(&mut app, &mut io, window_id, list_position);
    common::control_key(&mut app, &mut io, window_id, Key::Character("k"));
    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, PREVIEW), "banana contents");
    app.assert_invariants();
}

#[test]
fn ctrl_ik_move_the_selection_from_any_editor() {
    let (mut app, mut io, window_id) = open_file_app(&[
        ("/apple.txt", "apple contents"),
        ("/banana.txt", "banana contents"),
    ]);
    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, PREVIEW), "apple contents");

    // The path editor is focused, but ctrl+i/k go to the list editor.
    common::control_key(&mut app, &mut io, window_id, Key::Character("k"));
    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, PREVIEW), "banana contents");

    common::control_key(&mut app, &mut io, window_id, Key::Character("i"));
    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, PREVIEW), "apple contents");

    // The path editor still receives ordinary typing.
    common::char_input(&mut app, &mut io, window_id, 'x');
    assert_eq!(buffer_text(&app, PATH), "/x");
    app.assert_invariants();
}

#[test]
fn typing_into_the_list_does_nothing_and_selection_still_moves() {
    let (mut app, mut io, window_id) = open_file_app(&[
        ("/apple.txt", "apple contents"),
        ("/banana.txt", "banana contents"),
    ]);
    common::tick(&mut app, &mut io);
    let list_before = buffer_text(&app, LIST);

    // Focus the generated list and try to edit it.
    common::draw(&mut app, window_id, 40, 20);
    let cell_size = app.cell_size();
    let list_position = [2.0 * cell_size[0] as f32, 16.0 * cell_size[1] as f32];
    common::mouse_enter(&mut app, &mut io, window_id);
    common::mouse_moved(&mut app, &mut io, window_id, list_position);
    common::char_input(&mut app, &mut io, window_id, 'X');
    assert_eq!(buffer_text(&app, LIST), list_before);

    // Navigation remains available on generated buffers.
    common::control_key(&mut app, &mut io, window_id, Key::Character("k"));
    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, PREVIEW), "banana contents");
    assert_eq!(buffer_text(&app, LIST), list_before);
    app.assert_invariants();
}

#[test]
fn preview_reloads_with_cursor_at_top() {
    let (mut app, mut io, window_id) = open_file_app(&[
        ("/apple.txt", "apple contents"),
        ("/banana.txt", "banana contents"),
    ]);
    common::tick(&mut app, &mut io);
    common::control_key(&mut app, &mut io, window_id, Key::Character("k"));
    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, PREVIEW), "banana contents");

    // Focus the preview editor (the top half of the page): its cursor is
    // drawn on the first row, so it was reset to the top. The preview is
    // generated, so typing into it changes nothing.
    common::draw(&mut app, window_id, 40, 20);
    let cell_size = app.cell_size();
    let preview_position = [2.0 * cell_size[0] as f32, 2.0 * cell_size[1] as f32];
    common::mouse_enter(&mut app, &mut io, window_id);
    common::mouse_moved(&mut app, &mut io, window_id, preview_position);
    common::char_input(&mut app, &mut io, window_id, 'X');
    assert_eq!(buffer_text(&app, PREVIEW), "banana contents");
    let drawing = common::draw(&mut app, window_id, 40, 20);
    assert_eq!(common::cursor_lines(cell_size, &drawing), vec![0]);
    app.assert_invariants();
}

#[test]
fn opened_file_starts_at_top() {
    let (mut app, mut io, window_id) = open_file_app(&[("/dir/inner.txt", "inner contents")]);
    common::text_input(&mut app, &mut io, window_id, "dir/");
    common::tick(&mut app, &mut io);
    let buffers_before = app.buffers.keys().count();

    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::tick(&mut app, &mut io); // loads the file
    common::tick(&mut app, &mut io);
    // Typing lands at the start, so the cursor was at the top after loading.
    common::char_input(&mut app, &mut io, window_id, 'X');
    assert_eq!(buffer_text(&app, buffers_before), "Xinner contents");
    app.assert_invariants();
}

// Rows (in cells, from the top of the window) of the `>` markers drawn in
// the list gutter.
fn marker_rows(app: &App, drawing: &Drawing) -> Vec<usize> {
    let cell_h = app.cell_size()[1] as f32;
    drawing
        .commands
        .iter()
        .filter_map(|command| {
            let DrawCommand::Character(c) = command else {
                return None;
            };
            (c.ch == '>' && c.color == HIGHLIGHT_COLOR)
                .then(|| (c.dst.pos[1] / cell_h).round() as usize)
        })
        .collect()
}

#[test]
fn list_gutter_marks_selected_line() {
    let (mut app, mut io, window_id) = open_file_app(&[
        ("/apple.txt", "apple contents"),
        ("/banana.txt", "banana contents"),
    ]);
    common::tick(&mut app, &mut io);
    let drawing = common::draw(&mut app, window_id, 40, 20);
    let rows = marker_rows(&app, &drawing);
    assert_eq!(rows.len(), 1);

    // Moving the selection moves the marker down a line.
    common::control_key(&mut app, &mut io, window_id, Key::Character("k"));
    let drawing = common::draw(&mut app, window_id, 40, 20);
    assert_eq!(marker_rows(&app, &drawing), vec![rows[0] + 1]);
    app.assert_invariants();
}

#[test]
fn list_gutter_marks_nothing_without_a_selection() {
    let (mut app, mut io, window_id) = open_file_app(&[("/file.txt", "")]);
    common::text_input(&mut app, &mut io, window_id, "nomatch");
    common::tick(&mut app, &mut io);
    let drawing = common::draw(&mut app, window_id, 40, 20);
    assert_eq!(marker_rows(&app, &drawing), vec![]);
    app.assert_invariants();
}

#[test]
fn ctrl_enter_descends_into_dirs() {
    let (mut app, mut io, window_id) =
        open_file_app(&[("/dir/inner.txt", "inner contents"), ("/dab.txt", "")]);
    common::text_input(&mut app, &mut io, window_id, "di");
    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, LIST), "dir/");

    // Ctrl+enter on a dir appends it to the path and descends into it.
    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, PATH), "/dir/");
    assert_eq!(buffer_text(&app, LIST), "inner.txt");
    assert_eq!(buffer_text(&app, PREVIEW), "inner contents");

    // The cursor ends up after the completion, so typing extends it.
    common::char_input(&mut app, &mut io, window_id, 'x');
    assert_eq!(buffer_text(&app, PATH), "/dir/x");
    app.assert_invariants();
}

#[test]
fn ctrl_shift_enter_descends_in_new_window() {
    let (mut app, mut io, window_id) =
        open_file_app(&[("/dir/inner.txt", "inner contents"), ("/dab.txt", "")]);
    common::text_input(&mut app, &mut io, window_id, "di");
    common::tick(&mut app, &mut io);

    common::control_shift_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    let window_id_new = *io.open_windows.last().unwrap();
    common::tick(&mut app, &mut io);
    common::char_input(&mut app, &mut io, window_id_new, 'x');
    common::char_input(&mut app, &mut io, window_id, 'y');

    assert_eq!(io.open_windows.len(), 2);
    assert_eq!(buffer_text(&app, PATH), "/diy");
    assert!(
        app.buffers
            .keys()
            .any(|buffer_id| buffer_id.text(&app) == b"/dir/x")
    );
    app.assert_invariants();
}

#[test]
fn ctrl_enter_does_nothing_without_a_selection() {
    let (mut app, mut io, window_id) = open_file_app(&[("/file.txt", "")]);
    common::text_input(&mut app, &mut io, window_id, "nomatch");
    common::tick(&mut app, &mut io);
    let buffers_before = app.buffers.keys().count();
    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, PATH), "/nomatch");
    assert_eq!(app.buffers.keys().count(), buffers_before);
    app.assert_invariants();
}

#[test]
fn plain_enter_goes_to_the_focused_editor() {
    let (mut app, mut io, window_id) = open_file_app(&[("/file.txt", "")]);
    common::key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    assert_eq!(buffer_text(&app, PATH), "/\n");
    app.assert_invariants();
}

#[test]
fn alt_enter_creates_and_opens_file() {
    let (mut app, mut io, window_id) = open_file_app(&[("/other.txt", "")]);
    common::text_input(&mut app, &mut io, window_id, "newdir/new.txt");
    let buffers_before = app.buffers.keys().count();

    common::alt_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::tick(&mut app, &mut io); // loads the file
    assert_eq!(
        io.files.get(&PathBuf::from("/newdir/new.txt")).unwrap().0,
        b""
    );
    assert_eq!(app.buffers.keys().count(), buffers_before + 2);
    common::tick(&mut app, &mut io);
    common::char_input(&mut app, &mut io, window_id, 'X');
    assert_eq!(buffer_text(&app, buffers_before), "X");
    app.assert_invariants();
}

#[test]
fn alt_enter_rejects_a_relative_path() {
    // Minimized from a fuzzer failure: deleting the leading `/` and creating
    // gave a buffer whose path was relative to the editor's cwd.
    let (mut app, mut io, window_id) = open_file_app(&[("/dir/a.txt", "")]);
    common::key(
        &mut app,
        &mut io,
        window_id,
        Key::Named(NamedKey::Backspace),
    );
    common::text_input(&mut app, &mut io, window_id, "dir/new.txt");
    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, PATH), "dir/new.txt");
    assert_eq!(buffer_text(&app, LIST), "dir/: not an absolute path");

    common::alt_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::tick(&mut app, &mut io);

    // Nothing created or opened; still on the picker.
    assert!(!io.files.contains_key(&PathBuf::from("dir/new.txt")));
    assert_eq!(app.buffers.keys().count(), LIST + 1);
    common::char_input(&mut app, &mut io, window_id, 'x');
    assert_eq!(buffer_text(&app, PATH), "dir/new.txtx");
    app.assert_invariants();
}

#[test]
fn alt_enter_opens_existing_file_without_truncating() {
    let (mut app, mut io, window_id) = open_file_app(&[("/file.txt", "contents")]);
    common::text_input(&mut app, &mut io, window_id, "file.txt");
    let buffers_before = app.buffers.keys().count();

    common::alt_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::tick(&mut app, &mut io);
    common::tick(&mut app, &mut io);
    assert_eq!(
        io.files.get(&PathBuf::from("/file.txt")).unwrap().0,
        b"contents"
    );
    assert_eq!(buffer_text(&app, buffers_before), "contents");
    app.assert_invariants();
}

#[test]
fn alt_enter_does_nothing_without_a_file_name() {
    let (mut app, mut io, window_id) = open_file_app(&[("/file.txt", "")]);
    let buffers_before = app.buffers.keys().count();
    common::alt_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::tick(&mut app, &mut io);
    assert_eq!(app.buffers.keys().count(), buffers_before);
    assert_eq!(io.files.len(), 1);
    app.assert_invariants();
}

#[test]
fn ctrl_enter_opens_selected_file() {
    let (mut app, mut io, window_id) = open_file_app(&[("/dir/inner.txt", "inner contents")]);
    common::text_input(&mut app, &mut io, window_id, "dir/");
    common::tick(&mut app, &mut io);
    let buffers_before = app.buffers.keys().count();

    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::tick(&mut app, &mut io); // loads the file
    // A file buffer and a status bar buffer for the new edit page.
    assert_eq!(app.buffers.keys().count(), buffers_before + 2);
    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, buffers_before), "inner contents");
    app.assert_invariants();
}

#[test]
fn ctrl_q_after_opened_file_skips_file_picker() {
    let (mut app, mut io, window_id) = open_file_app(&[("/dir/inner.txt", "inner contents")]);
    common::text_input(&mut app, &mut io, window_id, "dir/");
    common::tick(&mut app, &mut io);
    let file_buffer = app.buffers.keys().count();

    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::tick(&mut app, &mut io);
    common::tick(&mut app, &mut io);
    common::char_input(&mut app, &mut io, window_id, 'X');
    common::control_key(&mut app, &mut io, window_id, Key::Character("q"));
    common::char_input(&mut app, &mut io, window_id, 'z');

    assert_eq!(buffer_text(&app, file_buffer), "Xinner contents");
    assert_eq!(buffer_text(&app, PATH), "/dir/");
    assert_eq!(common::text(&app), "z");
    app.assert_invariants();
}

#[test]
fn ctrl_enter_does_nothing_for_dirs() {
    let (mut app, mut io, window_id) = open_file_app(&[("/dir/inner.txt", "")]);
    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, LIST), "dir/");
    let buffers_before = app.buffers.keys().count();
    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::tick(&mut app, &mut io);
    assert_eq!(app.buffers.keys().count(), buffers_before);
    app.assert_invariants();
}

#[test]
fn ctrl_enter_reuses_existing_buffer() {
    let path = PathBuf::from("/file.txt");
    let (mut app, mut io, window_id) = common::file_app(path, "contents");
    common::control_key(&mut app, &mut io, window_id, Key::Character("o"));
    common::tick(&mut app, &mut io);
    let buffers_before = app.buffers.keys().count();

    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::tick(&mut app, &mut io);
    // Only a status bar buffer is created; the file buffer is reused.
    assert_eq!(app.buffers.keys().count(), buffers_before + 1);
    app.assert_invariants();
}
