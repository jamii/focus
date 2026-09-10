use std::path::PathBuf;

use focus_core::app::App;
use focus_core::drawing::{DrawCommand, Drawing};
use focus_core::input::{ButtonState, Key, ModifiersState, NamedKey};
use focus_core::style::HIGHLIGHT_COLOR;
use focus_core::window::WindowId;

mod common;

const TARGET: usize = 0;
const SEARCH: usize = 2;
const LIST: usize = 3;

fn buffer_text(app: &App, n: usize) -> String {
    app.buffers.keys().nth(n).unwrap().text(app).to_string()
}

fn search_buffer_app(text: &str) -> (App, focus_core::fuzz::MockIO, WindowId) {
    let (mut app, mut io, window_id) = common::file_app(PathBuf::from("/file.txt"), text);
    common::tick(&mut app, &mut io);
    common::control_key(&mut app, &mut io, window_id, Key::Character("f"));
    (app, io, window_id)
}

#[test]
fn empty_search_has_no_matches() {
    let (mut app, mut io, _window_id) = search_buffer_app("foo");

    common::tick(&mut app, &mut io);

    assert_eq!(buffer_text(&app, SEARCH), "");
    assert_eq!(buffer_text(&app, LIST), "");
    app.assert_invariants();
}

#[test]
fn lists_exact_case_sensitive_matches_with_line_number_prefix() {
    let (mut app, mut io, window_id) = search_buffer_app("Foo foo\naa aa\nbb aa");
    common::text_input(&mut app, &mut io, window_id, "aa");

    common::tick(&mut app, &mut io);

    assert_eq!(buffer_text(&app, LIST), "2 aa aa\n2 aa aa\n3 bb aa");
    app.assert_invariants();
}

#[test]
fn exact_match_is_case_sensitive() {
    let (mut app, mut io, window_id) = search_buffer_app("Foo foo");
    common::text_input(&mut app, &mut io, window_id, "foo");

    common::tick(&mut app, &mut io);

    assert_eq!(buffer_text(&app, LIST), "1 Foo foo");
    app.assert_invariants();
}

#[test]
fn ctrl_enter_opens_selected_match() {
    let (mut app, mut io, window_id) = search_buffer_app("one foo two foo");
    common::text_input(&mut app, &mut io, window_id, "foo");

    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::char_input(&mut app, &mut io, window_id, 'X');

    assert_eq!(buffer_text(&app, TARGET), "one X two foo");
    app.assert_invariants();
}

#[test]
fn opened_edit_scrolls_to_main_cursor() {
    let text = (0..20)
        .map(|i| {
            if i == 15 {
                "needle".to_string()
            } else {
                format!("line {i}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let (mut app, mut io, window_id) = search_buffer_app(&text);
    common::text_input(&mut app, &mut io, window_id, "needle");

    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    let drawing = common::draw(&mut app, window_id, 20, 6);

    assert!(!common::cursor_lines(app.cell_size(), &drawing).is_empty());
    app.assert_invariants();
}

#[test]
fn ctrl_f_marks_cached_search_text() {
    let (mut app, mut io, window_id) = search_buffer_app("foo bar");
    common::text_input(&mut app, &mut io, window_id, "foo");
    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));

    let buffers_before = app.buffers.keys().count();
    common::control_key(&mut app, &mut io, window_id, Key::Character("f"));
    common::text_input(&mut app, &mut io, window_id, "bar");

    assert_eq!(buffer_text(&app, buffers_before), "bar");
    app.assert_invariants();
}

#[test]
fn ctrl_enter_then_ctrl_f_selects_next_match() {
    let (mut app, mut io, window_id) = search_buffer_app("x foo one foo two foo");
    common::text_input(&mut app, &mut io, window_id, "foo");

    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::control_key(&mut app, &mut io, window_id, Key::Character("f"));
    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::char_input(&mut app, &mut io, window_id, 'X');

    assert_eq!(buffer_text(&app, TARGET), "x foo one X two foo");
    app.assert_invariants();
}

#[test]
fn alt_enter_opens_all_matches() {
    let (mut app, mut io, window_id) = search_buffer_app("foo one foo two foo");
    common::text_input(&mut app, &mut io, window_id, "foo");

    common::alt_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::char_input(&mut app, &mut io, window_id, 'X');

    assert_eq!(buffer_text(&app, TARGET), "X one X two X");
    app.assert_invariants();
}

#[test]
fn ctrl_enter_uses_all_bottom_cursors_and_keeps_duplicates() {
    let (mut app, mut io, window_id) = search_buffer_app("foo one foo");
    common::text_input(&mut app, &mut io, window_id, "foo");
    common::tick(&mut app, &mut io);

    common::draw(&mut app, window_id, 40, 20);
    let cell_size = app.cell_size();
    let line0 = [2.0 * cell_size[0] as f32, 11.5 * cell_size[1] as f32];
    let line1 = [2.0 * cell_size[0] as f32, 12.5 * cell_size[1] as f32];
    common::mouse_enter(&mut app, &mut io, window_id);
    common::mouse_moved(&mut app, &mut io, window_id, line0);
    common::mouse_button(&mut app, &mut io, window_id, ButtonState::Pressed, line0);
    common::mouse_button(&mut app, &mut io, window_id, ButtonState::Released, line0);
    common::modifiers(
        &mut app,
        &mut io,
        window_id,
        ModifiersState {
            control: true,
            ..Default::default()
        },
    );
    common::mouse_button(&mut app, &mut io, window_id, ButtonState::Pressed, line0);
    common::mouse_button(&mut app, &mut io, window_id, ButtonState::Released, line0);
    common::mouse_button(&mut app, &mut io, window_id, ButtonState::Pressed, line1);
    common::mouse_button(&mut app, &mut io, window_id, ButtonState::Released, line1);
    common::modifiers(&mut app, &mut io, window_id, ModifiersState::default());

    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::char_input(&mut app, &mut io, window_id, 'X');

    assert_eq!(buffer_text(&app, TARGET), "XX one X");
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
fn initial_selection_is_first_match_strictly_after_cursor() {
    let (mut app, mut io, window_id) = search_buffer_app("foo one foo two foo");
    common::text_input(&mut app, &mut io, window_id, "foo");
    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::control_key(&mut app, &mut io, window_id, Key::Character("f"));

    common::tick(&mut app, &mut io);
    let drawing = common::draw(&mut app, window_id, 40, 20);

    let rows = marker_rows(&app, &drawing);
    assert_eq!(rows.len(), 1);
    // The second result is selected, because the first match starts at the
    // original cursor offset and is not strictly after it.
    assert_eq!(rows[0], 13);
    app.assert_invariants();
}
