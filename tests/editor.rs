use focus::app::{App, WindowId};
use focus::fuzz::MockIO;
use winit::keyboard::{Key, NamedKey};

mod common;

fn move_right(app: &mut App, io: &mut MockIO, window_id: WindowId, count: usize) {
    for _ in 0..count {
        common::control_key(app, io, window_id, Key::Character("l".into()));
    }
}

fn move_left(app: &mut App, io: &mut MockIO, window_id: WindowId, count: usize) {
    for _ in 0..count {
        common::control_key(app, io, window_id, Key::Character("j".into()));
    }
}

fn select_right(app: &mut App, io: &mut MockIO, window_id: WindowId, count: usize) {
    common::control_key(app, io, window_id, Key::Named(NamedKey::Space));
    move_right(app, io, window_id, count);
}

#[test]
fn typing_and_cursor_movement_insert_at_the_cursor() {
    let (mut app, mut io, window_id) = common::scratch_app();

    common::text_input(&mut app, &mut io, window_id, "helo");
    common::control_key(&mut app, &mut io, window_id, Key::Character("j".into()));
    common::char_input(&mut app, &mut io, window_id, 'l');

    assert_eq!(common::text(&app), "hello");
    app.assert_invariants();
}

#[test]
fn copy_and_paste_use_mock_clipboard() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "hello");

    common::control_key(&mut app, &mut io, window_id, Key::Character("j".into()));
    common::control_key(&mut app, &mut io, window_id, Key::Character("j".into()));
    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Space));
    common::control_key(&mut app, &mut io, window_id, Key::Character("j".into()));
    common::control_key(&mut app, &mut io, window_id, Key::Character("j".into()));
    common::control_key(&mut app, &mut io, window_id, Key::Character("c".into()));
    common::control_key(&mut app, &mut io, window_id, Key::Character("l".into()));
    common::control_key(&mut app, &mut io, window_id, Key::Character("l".into()));
    common::control_key(&mut app, &mut io, window_id, Key::Character("v".into()));

    assert_eq!(io.clipboard, Some("el".to_string()));
    assert_eq!(common::text(&app), "helello");
    app.assert_invariants();
}

#[test]
fn cut_deletes_selection_and_updates_clipboard() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "hello");

    common::control_key(&mut app, &mut io, window_id, Key::Character("j".into()));
    common::control_key(&mut app, &mut io, window_id, Key::Character("j".into()));
    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Space));
    common::control_key(&mut app, &mut io, window_id, Key::Character("j".into()));
    common::control_key(&mut app, &mut io, window_id, Key::Character("j".into()));
    common::control_key(&mut app, &mut io, window_id, Key::Character("x".into()));

    assert_eq!(io.clipboard, Some("el".to_string()));
    assert_eq!(common::text(&app), "hlo");
    app.assert_invariants();
}

#[test]
fn ctrl_d_adds_next_match_for_replacement() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "abc abc abc");

    for _ in 0..8 {
        common::control_key(&mut app, &mut io, window_id, Key::Character("j".into()));
    }
    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Space));
    for _ in 0..3 {
        common::control_key(&mut app, &mut io, window_id, Key::Character("j".into()));
    }
    common::control_key(&mut app, &mut io, window_id, Key::Character("d".into()));
    common::char_input(&mut app, &mut io, window_id, 'X');

    assert_eq!(common::text(&app), "X X abc");
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
        common::text_line_lengths(&app, &drawing),
        Vec::<usize>::new()
    );
    assert_eq!(common::cursor_lines(&app, &drawing), vec![0]);
    app.assert_invariants();
}

#[test]
fn short_line_stays_on_one_wrap() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "abc");

    let drawing = common::draw(&mut app, window_id, 10, 3);

    assert_eq!(common::text_line_lengths(&app, &drawing), vec![3]);
    app.assert_invariants();
}

#[test]
fn newline_splits_into_two_wraps() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "ab\ncd");

    let drawing = common::draw(&mut app, window_id, 10, 4);

    assert_eq!(common::text_line_lengths(&app, &drawing), vec![2, 2]);
    app.assert_invariants();
}

#[test]
fn lone_newline_moves_cursor_to_empty_second_wrap() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "\n");

    let drawing = common::draw(&mut app, window_id, 10, 4);

    assert_eq!(
        common::text_line_lengths(&app, &drawing),
        Vec::<usize>::new()
    );
    assert_eq!(common::cursor_lines(&app, &drawing), vec![1]);
    app.assert_invariants();
}

#[test]
fn trailing_newline_leaves_empty_final_wrap() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "ab\n");

    let drawing = common::draw(&mut app, window_id, 10, 4);

    assert_eq!(common::text_line_lengths(&app, &drawing), vec![2]);
    assert_eq!(common::cursor_lines(&app, &drawing), vec![1]);
    app.assert_invariants();
}

#[test]
fn hard_wrap_when_no_space_available() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "abcde");

    let drawing = common::draw(&mut app, window_id, 3, 4);

    assert_eq!(common::text_line_lengths(&app, &drawing), vec![3, 2]);
    app.assert_invariants();
}

#[test]
fn soft_wrap_breaks_after_last_space() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "ab cdef");

    let drawing = common::draw(&mut app, window_id, 4, 4);

    assert_eq!(common::text_line_lengths(&app, &drawing), vec![3, 4]);
    app.assert_invariants();
}

#[test]
fn soft_wrap_does_not_reuse_earlier_space_after_overflow() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "ab cdefgh");

    let drawing = common::draw(&mut app, window_id, 4, 5);

    assert_eq!(common::text_line_lengths(&app, &drawing), vec![3, 4, 2]);
    app.assert_invariants();
}

#[test]
fn multi_byte_chars_count_as_one_grid_cell() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "héllo");

    let drawing = common::draw(&mut app, window_id, 10, 3);

    assert_eq!(common::text_line_lengths(&app, &drawing), vec![5]);
    app.assert_invariants();
}

#[test]
fn soft_wrap_before_multi_byte_char_keeps_it_intact() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "a b éef");

    let drawing = common::draw(&mut app, window_id, 3, 5);

    assert_eq!(common::text_line_lengths(&app, &drawing), vec![2, 2, 3]);
    app.assert_invariants();
}
