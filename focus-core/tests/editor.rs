use focus_core::app::{App, WindowId};
use focus_core::drawing::{DrawCommand, Drawing, FULL_BLOCK};
use focus_core::fuzz::MockIO;
use focus_core::input::{ElementState, Key, ModifiersState, NamedKey};
use focus_core::style::{BACKGROUND_COLOR, HIGHLIGHT_COLOR};

mod common;

fn move_right(app: &mut App, io: &mut MockIO, window_id: WindowId, count: usize) {
    for _ in 0..count {
        common::control_key(app, io, window_id, Key::Character("l"));
    }
}

fn move_left(app: &mut App, io: &mut MockIO, window_id: WindowId, count: usize) {
    for _ in 0..count {
        common::control_key(app, io, window_id, Key::Character("j"));
    }
}

fn select_right(app: &mut App, io: &mut MockIO, window_id: WindowId, count: usize) {
    common::control_key(app, io, window_id, Key::Named(NamedKey::Space));
    move_right(app, io, window_id, count);
}

fn move_down(app: &mut App, io: &mut MockIO, window_id: WindowId, count: usize) {
    for _ in 0..count {
        common::control_key(app, io, window_id, Key::Character("k"));
    }
}

fn solid_quads_with_color<'a>(
    drawing: &'a Drawing,
    color: [u8; 4],
) -> impl Iterator<Item = &'a focus_core::drawing::Character> {
    drawing.commands.iter().filter_map(move |command| {
        let DrawCommand::Character(c) = command else {
            return None;
        };
        // Solid fills are the Full Block in the given color.
        (c.ch == FULL_BLOCK && c.color == color).then_some(c)
    })
}

fn text_glyph_count_with_color(drawing: &Drawing, color: [u8; 4]) -> usize {
    drawing
        .commands
        .iter()
        .filter(|command| {
            let DrawCommand::Character(c) = command else {
                return false;
            };
            c.ch != FULL_BLOCK && c.color == color
        })
        .count()
}

fn clip_count(drawing: &Drawing) -> usize {
    drawing
        .commands
        .iter()
        .filter(|command| matches!(command, DrawCommand::SetClip(_)))
        .count()
}

#[test]
fn typing_and_cursor_movement_insert_at_the_cursor() {
    let (mut app, mut io, window_id) = common::scratch_app();

    common::text_input(&mut app, &mut io, window_id, "helo");
    common::control_key(&mut app, &mut io, window_id, Key::Character("j"));
    common::char_input(&mut app, &mut io, window_id, 'l');

    assert_eq!(common::text(&app), "hello");
    app.assert_invariants();
}

#[test]
fn unhandled_keys_and_released_keys_do_not_edit_text() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "ab");

    common::key(
        &mut app,
        &mut io,
        window_id,
        Key::Named(NamedKey::ArrowLeft),
    );
    common::modifiers(
        &mut app,
        &mut io,
        window_id,
        ModifiersState {
            control: true,
            ..Default::default()
        },
    );
    common::key(
        &mut app,
        &mut io,
        window_id,
        Key::Named(NamedKey::ArrowRight),
    );
    common::modifiers(&mut app, &mut io, window_id, ModifiersState::default());
    app.input(
        &mut io,
        window_id,
        focus_core::input::InputEvent::Key {
            state: ElementState::Released,
            logical_key: Key::Named(NamedKey::Backspace),
        },
    );

    assert_eq!(common::text(&app), "ab");
    app.assert_invariants();
}

#[test]
fn backspace_and_delete_noop_at_document_boundaries() {
    let (mut app, mut io, window_id) = common::scratch_app();

    common::key(
        &mut app,
        &mut io,
        window_id,
        Key::Named(NamedKey::Backspace),
    );
    common::key(&mut app, &mut io, window_id, Key::Named(NamedKey::Delete));
    assert_eq!(common::text(&app), "");

    common::char_input(&mut app, &mut io, window_id, 'a');
    move_left(&mut app, &mut io, window_id, 1);
    common::key(
        &mut app,
        &mut io,
        window_id,
        Key::Named(NamedKey::Backspace),
    );
    move_right(&mut app, &mut io, window_id, 1);
    common::key(&mut app, &mut io, window_id, Key::Named(NamedKey::Delete));

    assert_eq!(common::text(&app), "a");
    app.assert_invariants();
}

#[test]
fn clipboard_commands_noop_without_selection_or_clipboard_text() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "abc");

    io.clipboard = Some("old".into());
    common::control_key(&mut app, &mut io, window_id, Key::Character("c"));
    common::control_key(&mut app, &mut io, window_id, Key::Character("x"));

    assert_eq!(common::text(&app), "abc");
    assert_eq!(io.clipboard, Some("old".into()));

    io.clipboard = None;
    common::control_key(&mut app, &mut io, window_id, Key::Character("v"));
    common::control_key(&mut app, &mut io, window_id, Key::Character("V"));

    assert_eq!(common::text(&app), "abc");
    app.assert_invariants();
}

#[test]
fn copy_and_paste_use_mock_clipboard() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "hello");

    common::control_key(&mut app, &mut io, window_id, Key::Character("j"));
    common::control_key(&mut app, &mut io, window_id, Key::Character("j"));
    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Space));
    common::control_key(&mut app, &mut io, window_id, Key::Character("j"));
    common::control_key(&mut app, &mut io, window_id, Key::Character("j"));
    common::control_key(&mut app, &mut io, window_id, Key::Character("c"));
    common::control_key(&mut app, &mut io, window_id, Key::Character("l"));
    common::control_key(&mut app, &mut io, window_id, Key::Character("l"));
    common::control_key(&mut app, &mut io, window_id, Key::Character("v"));

    assert_eq!(io.clipboard, Some("el".into()));
    assert_eq!(common::text(&app), "helello");
    app.assert_invariants();
}

#[test]
fn cut_deletes_selection_and_updates_clipboard() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "hello");

    common::control_key(&mut app, &mut io, window_id, Key::Character("j"));
    common::control_key(&mut app, &mut io, window_id, Key::Character("j"));
    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Space));
    common::control_key(&mut app, &mut io, window_id, Key::Character("j"));
    common::control_key(&mut app, &mut io, window_id, Key::Character("j"));
    common::control_key(&mut app, &mut io, window_id, Key::Character("x"));

    assert_eq!(io.clipboard, Some("el".into()));
    assert_eq!(common::text(&app), "hlo");
    app.assert_invariants();
}

#[test]
fn delete_removes_character_to_the_right() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "abcd");

    move_left(&mut app, &mut io, window_id, 2);
    common::key(&mut app, &mut io, window_id, Key::Named(NamedKey::Delete));

    assert_eq!(common::text(&app), "abd");
    app.assert_invariants();
}

#[test]
fn delete_removes_selection() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "abcd");

    move_left(&mut app, &mut io, window_id, 3);
    select_right(&mut app, &mut io, window_id, 2);
    common::key(&mut app, &mut io, window_id, Key::Named(NamedKey::Delete));

    assert_eq!(common::text(&app), "ad");
    app.assert_invariants();
}

#[test]
fn overlapping_multi_cursor_selections_coalesce_when_deleted() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "abcdef");
    common::draw(&mut app, window_id, 10, 3);

    let start = common::point_for_offset(app.cell_size(), 1, 0);
    let end = common::point_for_offset(app.cell_size(), 4, 0);
    common::mouse_button(&mut app, &mut io, window_id, ElementState::Pressed, start);
    io.mouse_pos = end;
    common::tick(&mut app, &mut io);
    common::mouse_button(&mut app, &mut io, window_id, ElementState::Released, end);

    common::modifiers(
        &mut app,
        &mut io,
        window_id,
        ModifiersState {
            control: true,
            ..Default::default()
        },
    );
    let start = common::point_for_offset(app.cell_size(), 2, 0);
    let end = common::point_for_offset(app.cell_size(), 5, 0);
    common::mouse_button(&mut app, &mut io, window_id, ElementState::Pressed, start);
    io.mouse_pos = end;
    common::tick(&mut app, &mut io);
    common::mouse_button(&mut app, &mut io, window_id, ElementState::Released, end);
    common::modifiers(&mut app, &mut io, window_id, ModifiersState::default());

    common::key(
        &mut app,
        &mut io,
        window_id,
        Key::Named(NamedKey::Backspace),
    );

    assert_eq!(common::text(&app), "af");
    app.assert_invariants();
}

#[test]
fn delete_and_backspace_respect_multibyte_character_boundaries() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "hé猫");

    common::key(
        &mut app,
        &mut io,
        window_id,
        Key::Named(NamedKey::Backspace),
    );
    move_left(&mut app, &mut io, window_id, 1);
    common::key(&mut app, &mut io, window_id, Key::Named(NamedKey::Delete));

    assert_eq!(common::text(&app), "h");
    app.assert_invariants();
}

#[test]
fn ctrl_d_adds_next_match_for_replacement() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "abc abc abc");

    for _ in 0..8 {
        common::control_key(&mut app, &mut io, window_id, Key::Character("j"));
    }
    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Space));
    for _ in 0..3 {
        common::control_key(&mut app, &mut io, window_id, Key::Character("j"));
    }
    common::control_key(&mut app, &mut io, window_id, Key::Character("d"));
    common::char_input(&mut app, &mut io, window_id, 'X');

    assert_eq!(common::text(&app), "X X abc");
    app.assert_invariants();
}

#[test]
fn ctrl_d_noops_without_selection() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "abc abc");

    move_left(&mut app, &mut io, window_id, 3);
    common::control_key(&mut app, &mut io, window_id, Key::Character("d"));
    common::char_input(&mut app, &mut io, window_id, 'X');

    assert_eq!(common::text(&app), "abc Xabc");
    app.assert_invariants();
}

#[test]
fn ctrl_d_preserves_reverse_selection_direction() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "abc abc");

    move_left(&mut app, &mut io, window_id, 4);
    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Space));
    move_left(&mut app, &mut io, window_id, 3);
    common::control_key(&mut app, &mut io, window_id, Key::Character("d"));
    common::char_input(&mut app, &mut io, window_id, 'X');

    assert_eq!(common::text(&app), "X X");
    app.assert_invariants();
}

#[test]
fn ctrl_shift_d_removes_last_cursor() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "abc abc abc");

    move_left(&mut app, &mut io, window_id, 11);
    select_right(&mut app, &mut io, window_id, 3);
    common::control_key(&mut app, &mut io, window_id, Key::Character("d"));
    common::control_key(&mut app, &mut io, window_id, Key::Character("D"));
    common::char_input(&mut app, &mut io, window_id, 'X');

    assert_eq!(common::text(&app), "X abc abc");
    app.assert_invariants();
}

#[test]
fn paste_many_distributes_clipboard_lines_across_cursors() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "ab cd");

    move_left(&mut app, &mut io, window_id, 5);
    let first = common::point_for_offset(app.cell_size(), 0, 0);
    let second = common::point_for_offset(app.cell_size(), 3, 0);
    common::mouse_button(&mut app, &mut io, window_id, ElementState::Pressed, first);
    common::mouse_button(&mut app, &mut io, window_id, ElementState::Released, first);
    common::modifiers(
        &mut app,
        &mut io,
        window_id,
        ModifiersState {
            control: true,
            ..Default::default()
        },
    );
    common::mouse_button(&mut app, &mut io, window_id, ElementState::Pressed, second);
    common::mouse_button(&mut app, &mut io, window_id, ElementState::Released, second);
    common::modifiers(&mut app, &mut io, window_id, ModifiersState::default());

    io.clipboard = Some("X\nY".into());
    common::control_key(&mut app, &mut io, window_id, Key::Character("V"));

    assert_eq!(common::text(&app), "Xab Ycd");
    app.assert_invariants();
}

#[test]
fn paste_many_uses_available_clipboard_lines_and_ignores_extra_lines() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "ab cd");
    common::draw(&mut app, window_id, 10, 3);

    let first = common::point_for_offset(app.cell_size(), 0, 0);
    let second = common::point_for_offset(app.cell_size(), 3, 0);
    common::mouse_button(&mut app, &mut io, window_id, ElementState::Pressed, first);
    common::mouse_button(&mut app, &mut io, window_id, ElementState::Released, first);
    common::modifiers(
        &mut app,
        &mut io,
        window_id,
        ModifiersState {
            control: true,
            ..Default::default()
        },
    );
    common::mouse_button(&mut app, &mut io, window_id, ElementState::Pressed, second);
    common::mouse_button(&mut app, &mut io, window_id, ElementState::Released, second);
    common::modifiers(&mut app, &mut io, window_id, ModifiersState::default());

    io.clipboard = Some("X".into());
    common::control_key(&mut app, &mut io, window_id, Key::Character("V"));
    assert_eq!(common::text(&app), "Xab cd");

    move_left(&mut app, &mut io, window_id, 6);
    common::draw(&mut app, window_id, 10, 3);
    let first = common::point_for_offset(app.cell_size(), 0, 0);
    let second = common::point_for_offset(app.cell_size(), 4, 0);
    common::mouse_button(&mut app, &mut io, window_id, ElementState::Pressed, first);
    common::mouse_button(&mut app, &mut io, window_id, ElementState::Released, first);
    common::modifiers(
        &mut app,
        &mut io,
        window_id,
        ModifiersState {
            control: true,
            ..Default::default()
        },
    );
    common::mouse_button(&mut app, &mut io, window_id, ElementState::Pressed, second);
    common::mouse_button(&mut app, &mut io, window_id, ElementState::Released, second);
    common::modifiers(&mut app, &mut io, window_id, ModifiersState::default());

    io.clipboard = Some("1\n2\n3".into());
    common::control_key(&mut app, &mut io, window_id, Key::Character("V"));

    assert_eq!(common::text(&app), "1Xab 2cd");
    app.assert_invariants();
}

#[test]
fn vertical_movement_preserves_wanted_column() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "abcd\nef\nwxyz");

    common::alt_key(&mut app, &mut io, window_id, Key::Character("i"));
    move_right(&mut app, &mut io, window_id, 3);
    move_down(&mut app, &mut io, window_id, 1);
    move_down(&mut app, &mut io, window_id, 1);
    common::char_input(&mut app, &mut io, window_id, 'Q');

    assert_eq!(common::text(&app), "abcd\nef\nwxyQz");
    app.assert_invariants();
}

#[test]
fn vertical_movement_across_soft_wraps() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "abcdef");
    common::draw(&mut app, window_id, 3, 4);

    common::alt_key(&mut app, &mut io, window_id, Key::Character("i"));
    move_right(&mut app, &mut io, window_id, 1);
    move_down(&mut app, &mut io, window_id, 1);
    common::char_input(&mut app, &mut io, window_id, 'Q');

    assert_eq!(common::text(&app), "abcdQef");
    app.assert_invariants();
}

#[test]
fn alt_navigation_moves_to_line_and_document_boundaries() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "abc\ndef");

    common::alt_key(&mut app, &mut io, window_id, Key::Character("i"));
    common::char_input(&mut app, &mut io, window_id, 'A');
    common::alt_key(&mut app, &mut io, window_id, Key::Character("k"));
    common::char_input(&mut app, &mut io, window_id, 'Z');
    move_left(&mut app, &mut io, window_id, 2);
    common::alt_key(&mut app, &mut io, window_id, Key::Character("j"));
    common::char_input(&mut app, &mut io, window_id, 'L');
    common::alt_key(&mut app, &mut io, window_id, Key::Character("l"));
    common::char_input(&mut app, &mut io, window_id, 'R');

    assert_eq!(common::text(&app), "Aabc\nLdefZR");
    app.assert_invariants();
}

#[test]
fn click_places_cursor_at_text_offset() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "abcd");
    common::draw(&mut app, window_id, 10, 3);

    let point = common::point_for_offset(app.cell_size(), 2, 0);
    common::mouse_button(&mut app, &mut io, window_id, ElementState::Pressed, point);
    common::mouse_button(&mut app, &mut io, window_id, ElementState::Released, point);
    common::char_input(&mut app, &mut io, window_id, 'X');

    assert_eq!(common::text(&app), "abXcd");
    app.assert_invariants();
}

#[test]
fn click_hit_testing_handles_screen_edges_and_half_cells() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "abcd");
    common::draw(&mut app, window_id, 10, 3);

    common::mouse_button(
        &mut app,
        &mut io,
        window_id,
        ElementState::Pressed,
        [0.0, -10.0],
    );
    common::mouse_button(
        &mut app,
        &mut io,
        window_id,
        ElementState::Released,
        [0.0, -10.0],
    );
    common::char_input(&mut app, &mut io, window_id, 'A');
    assert_eq!(common::text(&app), "Aabcd");

    common::draw(&mut app, window_id, 10, 3);
    let left_gutter = [0.0, app.cell_size()[1] as f32 / 2.0];
    common::mouse_button(
        &mut app,
        &mut io,
        window_id,
        ElementState::Pressed,
        left_gutter,
    );
    common::mouse_button(
        &mut app,
        &mut io,
        window_id,
        ElementState::Released,
        left_gutter,
    );
    common::char_input(&mut app, &mut io, window_id, 'B');
    assert_eq!(common::text(&app), "BAabcd");

    common::draw(&mut app, window_id, 10, 3);
    let cell_w = app.cell_size()[0] as f32;
    let cell_h = app.cell_size()[1] as f32;
    let right_half_of_second_char = [cell_w * 3.75, cell_h / 2.0];
    common::mouse_button(
        &mut app,
        &mut io,
        window_id,
        ElementState::Pressed,
        right_half_of_second_char,
    );
    common::mouse_button(
        &mut app,
        &mut io,
        window_id,
        ElementState::Released,
        right_half_of_second_char,
    );
    common::char_input(&mut app, &mut io, window_id, 'C');
    assert_eq!(common::text(&app), "BAaCbcd");

    common::draw(&mut app, window_id, 10, 3);
    let below_document = [cell_w, cell_h * 10.0];
    common::mouse_button(
        &mut app,
        &mut io,
        window_id,
        ElementState::Pressed,
        below_document,
    );
    common::mouse_button(
        &mut app,
        &mut io,
        window_id,
        ElementState::Released,
        below_document,
    );
    common::char_input(&mut app, &mut io, window_id, 'D');

    assert_eq!(common::text(&app), "BAaCbcdD");
    app.assert_invariants();
}

#[test]
fn ctrl_click_adds_another_cursor() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "abcd");
    common::draw(&mut app, window_id, 10, 3);

    let first = common::point_for_offset(app.cell_size(), 1, 0);
    common::mouse_button(&mut app, &mut io, window_id, ElementState::Pressed, first);
    common::mouse_button(&mut app, &mut io, window_id, ElementState::Released, first);
    common::modifiers(
        &mut app,
        &mut io,
        window_id,
        ModifiersState {
            control: true,
            ..Default::default()
        },
    );
    let second = common::point_for_offset(app.cell_size(), 3, 0);
    common::mouse_button(&mut app, &mut io, window_id, ElementState::Pressed, second);
    common::mouse_button(&mut app, &mut io, window_id, ElementState::Released, second);
    common::modifiers(&mut app, &mut io, window_id, ModifiersState::default());
    common::char_input(&mut app, &mut io, window_id, 'X');

    assert_eq!(common::text(&app), "aXbcXd");
    app.assert_invariants();
}

#[test]
fn dragging_selects_text_for_replacement() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "abcd");
    common::draw(&mut app, window_id, 10, 3);

    let start = common::point_for_offset(app.cell_size(), 1, 0);
    let end = common::point_for_offset(app.cell_size(), 3, 0);
    common::mouse_button(&mut app, &mut io, window_id, ElementState::Pressed, start);
    io.mouse_pos = end;
    common::tick(&mut app, &mut io);
    common::mouse_button(&mut app, &mut io, window_id, ElementState::Released, end);
    common::char_input(&mut app, &mut io, window_id, 'X');

    assert_eq!(common::text(&app), "aXd");
    app.assert_invariants();
}

#[test]
fn mouse_wheel_scrolls_visible_wraps() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "a\nbb\nccc\ndddd\neeeee");

    let drawing = common::draw(&mut app, window_id, 10, 3);
    let before = common::text_line_lengths(app.cell_size(), &drawing);
    common::mouse_wheel(&mut app, &mut io, window_id, -1.0);
    let drawing = common::draw(&mut app, window_id, 10, 3);
    let after = common::text_line_lengths(app.cell_size(), &drawing);

    assert_ne!(after, before);
    app.assert_invariants();
}

#[test]
fn dragging_below_viewport_scrolls_visible_wraps() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "a\nbb\nccc\ndddd\neeeee");

    let drawing = common::draw(&mut app, window_id, 10, 3);
    let before = common::text_line_lengths(app.cell_size(), &drawing);
    let start = common::point_for_offset(app.cell_size(), 0, 0);
    common::mouse_button(&mut app, &mut io, window_id, ElementState::Pressed, start);
    io.mouse_pos = common::point_for_offset(app.cell_size(), 0, 5);
    common::tick(&mut app, &mut io);
    let drawing = common::draw(&mut app, window_id, 10, 3);
    let after = common::text_line_lengths(app.cell_size(), &drawing);
    let release = io.mouse_pos;
    common::mouse_button(
        &mut app,
        &mut io,
        window_id,
        ElementState::Released,
        release,
    );

    assert_ne!(after, before);
    app.assert_invariants();
}

#[test]
fn dragging_above_viewport_scrolls_visible_wraps() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "a\nbb\nccc\ndddd\neeeee");

    common::mouse_wheel(&mut app, &mut io, window_id, -10.0);
    let drawing = common::draw(&mut app, window_id, 10, 3);
    let before = common::text_line_lengths(app.cell_size(), &drawing);
    let start = common::point_for_offset(app.cell_size(), 0, 2);
    common::mouse_button(&mut app, &mut io, window_id, ElementState::Pressed, start);
    io.mouse_pos = [0.0, -20.0];
    common::tick(&mut app, &mut io);
    let drawing = common::draw(&mut app, window_id, 10, 3);
    let after = common::text_line_lengths(app.cell_size(), &drawing);
    let release = io.mouse_pos;
    common::mouse_button(
        &mut app,
        &mut io,
        window_id,
        ElementState::Released,
        release,
    );

    assert_ne!(after, before);
    app.assert_invariants();
}

#[test]
fn cursor_shifts_through_earlier_delete_from_another_editor() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "abcdefghijk");
    move_left(&mut app, &mut io, window_id, 1);
    let other_window_id = common::open_same_document_window(&mut app, &mut io, window_id);

    move_right(&mut app, &mut io, other_window_id, 4);
    common::key(
        &mut app,
        &mut io,
        other_window_id,
        Key::Named(NamedKey::Backspace),
    );
    common::char_input(&mut app, &mut io, window_id, 'X');

    assert_eq!(common::text(&app), "abcefghijXk");
    app.assert_invariants();
}

#[test]
fn cursor_ignores_later_insert_from_another_editor() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "abcdefghijk");
    move_left(&mut app, &mut io, window_id, 6);
    let other_window_id = common::open_same_document_window(&mut app, &mut io, window_id);

    move_right(&mut app, &mut io, other_window_id, 10);
    common::char_input(&mut app, &mut io, other_window_id, 'X');
    common::char_input(&mut app, &mut io, window_id, 'Z');

    assert_eq!(common::text(&app), "abcdeZfghijXk");
    app.assert_invariants();
}

#[test]
fn cursor_inside_delete_from_another_editor_clamps_to_delete_start() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "abcdefghij");
    move_left(&mut app, &mut io, window_id, 5);
    let other_window_id = common::open_same_document_window(&mut app, &mut io, window_id);

    move_right(&mut app, &mut io, other_window_id, 3);
    select_right(&mut app, &mut io, other_window_id, 4);
    common::key(
        &mut app,
        &mut io,
        other_window_id,
        Key::Named(NamedKey::Backspace),
    );
    common::char_input(&mut app, &mut io, window_id, 'X');

    assert_eq!(common::text(&app), "abcXhij");
    app.assert_invariants();
}

#[test]
fn cursor_shifts_through_earlier_insert_from_another_editor() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "abcdefghijklmnop");
    move_left(&mut app, &mut io, window_id, 6);
    let other_window_id = common::open_same_document_window(&mut app, &mut io, window_id);

    move_right(&mut app, &mut io, other_window_id, 2);
    common::text_input(&mut app, &mut io, other_window_id, "XYZ");
    common::char_input(&mut app, &mut io, window_id, 'Q');

    assert_eq!(common::text(&app), "abXYZcdefghijQklmnop");
    app.assert_invariants();
}

#[test]
fn selection_shifts_through_earlier_insert_from_another_editor() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "abcdefghijklmnop");
    move_left(&mut app, &mut io, window_id, 10);
    select_right(&mut app, &mut io, window_id, 4);
    let other_window_id = common::open_same_document_window(&mut app, &mut io, window_id);

    move_right(&mut app, &mut io, other_window_id, 2);
    common::text_input(&mut app, &mut io, other_window_id, "XYZ");
    common::char_input(&mut app, &mut io, window_id, 'Q');

    assert_eq!(common::text(&app), "abXYZcdefQklmnop");
    app.assert_invariants();
}

#[test]
fn selection_inside_delete_from_another_editor_collapses_to_delete_start() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "abcdefghij");
    move_left(&mut app, &mut io, window_id, 5);
    select_right(&mut app, &mut io, window_id, 2);
    let other_window_id = common::open_same_document_window(&mut app, &mut io, window_id);

    move_right(&mut app, &mut io, other_window_id, 3);
    select_right(&mut app, &mut io, other_window_id, 6);
    common::key(
        &mut app,
        &mut io,
        other_window_id,
        Key::Named(NamedKey::Backspace),
    );
    common::char_input(&mut app, &mut io, window_id, 'Q');

    assert_eq!(common::text(&app), "abcQj");
    app.assert_invariants();
}

#[test]
fn empty_text_draws_cursor_on_first_wrap() {
    let (mut app, _, window_id) = common::scratch_app();

    let drawing = common::draw(&mut app, window_id, 10, 3);

    assert_eq!(
        common::text_line_lengths(app.cell_size(), &drawing),
        Vec::<usize>::new()
    );
    assert_eq!(common::cursor_lines(app.cell_size(), &drawing), vec![0]);
    app.assert_invariants();
}

#[test]
fn short_line_stays_on_one_wrap() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "abc");

    let drawing = common::draw(&mut app, window_id, 10, 3);

    assert_eq!(common::text_line_lengths(app.cell_size(), &drawing), vec![3]);
    app.assert_invariants();
}

#[test]
fn newline_splits_into_two_wraps() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "ab\ncd");

    let drawing = common::draw(&mut app, window_id, 10, 4);

    assert_eq!(common::text_line_lengths(app.cell_size(), &drawing), vec![2, 2]);
    app.assert_invariants();
}

#[test]
fn lone_newline_moves_cursor_to_empty_second_wrap() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "\n");

    let drawing = common::draw(&mut app, window_id, 10, 4);

    assert_eq!(
        common::text_line_lengths(app.cell_size(), &drawing),
        Vec::<usize>::new()
    );
    assert_eq!(common::cursor_lines(app.cell_size(), &drawing), vec![1]);
    app.assert_invariants();
}

#[test]
fn trailing_newline_leaves_empty_final_wrap() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "ab\n");

    let drawing = common::draw(&mut app, window_id, 10, 4);

    assert_eq!(common::text_line_lengths(app.cell_size(), &drawing), vec![2]);
    assert_eq!(common::cursor_lines(app.cell_size(), &drawing), vec![1]);
    app.assert_invariants();
}

#[test]
fn hard_wrap_when_no_space_available() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "abcde");

    let drawing = common::draw(&mut app, window_id, 3, 4);

    assert_eq!(common::text_line_lengths(app.cell_size(), &drawing), vec![3, 2]);
    app.assert_invariants();
}

#[test]
fn soft_wrap_breaks_after_last_space() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "ab cdef");

    let drawing = common::draw(&mut app, window_id, 4, 4);

    assert_eq!(common::text_line_lengths(app.cell_size(), &drawing), vec![3, 4]);
    app.assert_invariants();
}

#[test]
fn soft_wrap_does_not_reuse_earlier_space_after_overflow() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "ab cdefgh");

    let drawing = common::draw(&mut app, window_id, 4, 5);

    assert_eq!(common::text_line_lengths(app.cell_size(), &drawing), vec![3, 4, 2]);
    app.assert_invariants();
}

#[test]
fn multi_byte_chars_count_as_one_grid_cell() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "héllo");

    let drawing = common::draw(&mut app, window_id, 10, 3);

    assert_eq!(common::text_line_lengths(app.cell_size(), &drawing), vec![5]);
    app.assert_invariants();
}

#[test]
fn soft_wrap_before_multi_byte_char_keeps_it_intact() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "a b éef");

    let drawing = common::draw(&mut app, window_id, 3, 5);

    assert_eq!(common::text_line_lengths(app.cell_size(), &drawing), vec![2, 2, 3]);
    app.assert_invariants();
}

#[test]
fn tiny_viewport_draws_only_window_background() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "abc");

    let cell_w = app.cell_size()[0] as f32;
    let cell_h = app.cell_size()[1] as f32;
    let mut drawing = Drawing::new([cell_w * 2.0, cell_h * 3.0]);
    app.draw(window_id, &mut drawing);

    assert_eq!(
        common::text_line_lengths(app.cell_size(), &drawing),
        Vec::<usize>::new()
    );
    assert_eq!(common::cursor_lines(app.cell_size(), &drawing), Vec::<usize>::new());
    assert_eq!(
        solid_quads_with_color(&drawing, BACKGROUND_COLOR).count(),
        1
    );
    app.assert_invariants();
}

#[test]
fn draw_emits_soft_wrap_gutter_selection_highlight_and_scrollbar() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "ab cdef");
    common::draw(&mut app, window_id, 4, 4);

    move_left(&mut app, &mut io, window_id, 6);
    select_right(&mut app, &mut io, window_id, 5);
    let drawing = common::draw(&mut app, window_id, 4, 4);

    assert_eq!(
        text_glyph_count_with_color(&drawing, HIGHLIGHT_COLOR),
        1
    );
    assert!(solid_quads_with_color(&drawing, HIGHLIGHT_COLOR).count() >= 2);
    assert!(solid_quads_with_color(&drawing, BACKGROUND_COLOR).count() >= 2);
    app.assert_invariants();
}

#[test]
fn partially_clipped_text_uses_scissor_commands() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "a\nbb\nccc\ndddd\neeeee");

    common::draw(&mut app, window_id, 10, 3);
    common::mouse_wheel(&mut app, &mut io, window_id, -0.1);
    let drawing = common::draw(&mut app, window_id, 10, 3);

    assert!(clip_count(&drawing) > 0);
    app.assert_invariants();
}

#[test]
fn cursor_blinks_off_after_idle_time() {
    let (mut app, mut io, window_id) = common::scratch_app();
    let drawing = common::draw(&mut app, window_id, 10, 3);
    assert_eq!(common::cursor_lines(app.cell_size(), &drawing), vec![0]);

    io.frame_start += std::time::Duration::from_millis(600);
    common::tick(&mut app, &mut io);
    let drawing = common::draw(&mut app, window_id, 10, 3);

    assert_eq!(common::cursor_lines(app.cell_size(), &drawing), Vec::<usize>::new());
    app.assert_invariants();
}

#[test]
fn resizing_viewport_preserves_centered_content() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(
        &mut app,
        &mut io,
        window_id,
        "a\nbb\nccc\ndddd\neeeee\nffffff",
    );

    common::mouse_wheel(&mut app, &mut io, window_id, -3.0);
    let before = common::draw(&mut app, window_id, 10, 3);
    let before_lines = common::text_line_lengths(app.cell_size(), &before);

    let after = common::draw(&mut app, window_id, 10, 6);
    let after_lines = common::text_line_lengths(app.cell_size(), &after);

    assert!(before_lines.iter().any(|line| after_lines.contains(line)));
    app.assert_invariants();
}

#[test]
fn ctrl_d_noops_when_selection_has_no_later_match() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "abc abc");

    move_left(&mut app, &mut io, window_id, 3);
    select_right(&mut app, &mut io, window_id, 3);
    common::control_key(&mut app, &mut io, window_id, Key::Character("d"));
    common::char_input(&mut app, &mut io, window_id, 'X');

    assert_eq!(common::text(&app), "abc X");
    app.assert_invariants();
}

#[test]
fn ctrl_shift_d_noops_with_one_cursor() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "abc");

    common::control_key(&mut app, &mut io, window_id, Key::Character("D"));
    common::char_input(&mut app, &mut io, window_id, 'X');

    assert_eq!(common::text(&app), "abcX");
    app.assert_invariants();
}

#[test]
fn ctrl_space_toggles_selection_off() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "abcd");

    move_left(&mut app, &mut io, window_id, 4);
    select_right(&mut app, &mut io, window_id, 2);
    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Space));
    common::char_input(&mut app, &mut io, window_id, 'X');

    assert_eq!(common::text(&app), "abXcd");
    app.assert_invariants();
}

#[test]
fn copy_joins_multiple_selections_with_newlines() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "abc abc");

    move_left(&mut app, &mut io, window_id, 7);
    select_right(&mut app, &mut io, window_id, 3);
    common::control_key(&mut app, &mut io, window_id, Key::Character("d"));
    common::control_key(&mut app, &mut io, window_id, Key::Character("c"));

    assert_eq!(io.clipboard, Some("abc\nabc".into()));
    assert_eq!(common::text(&app), "abc abc");
    app.assert_invariants();
}

#[test]
fn keys_with_control_and_alt_do_not_trigger_editor_shortcuts() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "ab");

    common::modifiers(
        &mut app,
        &mut io,
        window_id,
        ModifiersState {
            control: true,
            alt: true,
            ..ModifiersState::default()
        },
    );
    common::key(&mut app, &mut io, window_id, Key::Character("l"));
    common::modifiers(&mut app, &mut io, window_id, ModifiersState::default());
    common::char_input(&mut app, &mut io, window_id, 'X');

    assert_eq!(common::text(&app), "abX");
    app.assert_invariants();
}
