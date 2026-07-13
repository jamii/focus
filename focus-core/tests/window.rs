use focus_core::drawing::{DrawCommand, FULL_BLOCK};
use focus_core::input::{InputEvent, Key};
use focus_core::style::TEXT_COLOR;

mod common;

#[test]
fn ctrl_n_opens_a_new_window_with_a_new_buffer() {
    let (mut app, mut io, window_id) = common::scratch_app();

    let before = io.open_windows.clone();
    common::control_key(&mut app, &mut io, window_id, Key::Character("n"));
    let window_id_new = *io
        .open_windows
        .iter()
        .find(|window_id| !before.contains(window_id))
        .unwrap();
    common::char_input(&mut app, &mut io, window_id_new, 'X');

    assert_eq!(io.open_windows.len(), 2);
    assert_eq!(common::text(&app), "");
    app.assert_invariants();
}

#[test]
fn ctrl_m_opens_a_new_window_on_the_same_buffer() {
    let (mut app, mut io, window_id) = common::scratch_app();

    let window_id_new = common::open_same_buffer_window(&mut app, &mut io, window_id);
    common::char_input(&mut app, &mut io, window_id_new, 'X');

    assert_eq!(io.open_windows.len(), 2);
    assert_eq!(common::text(&app), "X");
    app.assert_invariants();
}

#[test]
fn ctrl_q_pops_back_to_previous_page() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "base");

    common::control_key(&mut app, &mut io, window_id, Key::Character("o"));
    common::char_input(&mut app, &mut io, window_id, 'x');
    common::control_key(&mut app, &mut io, window_id, Key::Character("q"));
    common::char_input(&mut app, &mut io, window_id, 'y');

    assert_eq!(common::text(&app), "basey");
    app.assert_invariants();
}

#[test]
fn ctrl_q_pops_back_from_repo_search() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "base");

    common::alt_key(&mut app, &mut io, window_id, Key::Character("f"));
    common::char_input(&mut app, &mut io, window_id, 'x');
    common::control_key(&mut app, &mut io, window_id, Key::Character("q"));
    common::char_input(&mut app, &mut io, window_id, 'y');

    assert_eq!(common::text(&app), "basey");
    app.assert_invariants();
}

#[test]
fn ctrl_q_on_last_page_leaves_a_scratch_page() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "base");

    common::control_key(&mut app, &mut io, window_id, Key::Character("q"));
    common::char_input(&mut app, &mut io, window_id, 'y');

    assert_eq!(io.open_windows, vec![window_id]);
    assert!(!io.exited);
    assert_eq!(common::text(&app), "base");
    assert!(
        app.buffers
            .keys()
            .any(|buffer_id| buffer_id.text(&app).to_string() == "y")
    );
    app.assert_invariants();
}

#[test]
fn closing_last_window_exits() {
    let (mut app, mut io, window_id) = common::scratch_app();

    app.input(&mut io, window_id, InputEvent::CloseRequested);

    assert!(io.open_windows.is_empty());
    assert!(io.exited);
}

#[test]
fn closing_a_window_autosaves() {
    let path = std::path::PathBuf::from("/tmp/focus-close-autosave-test.txt");
    let (mut app, mut io, window_id) = common::file_app(path.clone(), "before");
    common::tick(&mut app, &mut io);

    io.frame_start += std::time::Duration::from_secs(1);
    common::tick(&mut app, &mut io);
    common::alt_key(&mut app, &mut io, window_id, Key::Character("k"));
    common::text_input(&mut app, &mut io, window_id, " after");

    app.input(&mut io, window_id, InputEvent::CloseRequested);

    assert_eq!(io.files.get(&path).unwrap().0, b"before after");
    assert!(io.open_windows.is_empty());
    app.assert_invariants();
}

#[test]
fn draw_includes_status_bar_on_bottom_row() {
    let (mut app, mut io, window_id) = common::scratch_app();

    common::tick(&mut app, &mut io);
    let drawing = common::draw(&mut app, window_id, 10, 3);
    let cell_h = app.cell_size()[1] as f32;
    let bottom_row_text: String = drawing
        .commands
        .iter()
        .filter_map(|command| {
            let DrawCommand::Character(c) = command else {
                return None;
            };
            let row = (c.dst.pos[1] / cell_h).round() as usize;
            (row == 2 && c.color == TEXT_COLOR && c.ch != FULL_BLOCK).then_some(c.ch)
        })
        .collect();

    assert!(bottom_row_text.contains("scratch"));
}

#[test]
fn status_bar_shows_scratch_cursor_position() {
    let (mut app, mut io, _window_id) = common::scratch_app();

    common::tick(&mut app, &mut io);
    let status_text = app
        .buffers
        .keys()
        .map(|buffer_id| buffer_id.text(&app).to_string())
        .find(|text| text.starts_with("scratch"))
        .unwrap();

    assert_eq!(status_text, "scratch 1:1");
    app.assert_invariants();
}

#[test]
fn status_bar_updates_cursor_position_after_movement() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "abc\ndef");

    common::control_key(&mut app, &mut io, window_id, Key::Character("j"));
    common::control_key(&mut app, &mut io, window_id, Key::Character("j"));
    common::tick(&mut app, &mut io);
    let status_text = app
        .buffers
        .keys()
        .map(|buffer_id| buffer_id.text(&app).to_string())
        .find(|text| text.starts_with("scratch"))
        .unwrap();

    assert_eq!(status_text, "scratch 2:2");
    app.assert_invariants();
}

#[test]
fn status_bar_uses_file_path_for_file_buffers() {
    let path = std::path::PathBuf::from("/tmp/focus-status-path-test.txt");
    let (mut app, mut io, _window_id) = common::file_app(path.clone(), "abc");

    common::tick(&mut app, &mut io);
    let status_text = app
        .buffers
        .keys()
        .map(|buffer_id| buffer_id.text(&app).to_string())
        .find(|text| text.starts_with(path.to_str().unwrap()))
        .unwrap();

    // The cursor starts at the top after loading.
    assert_eq!(status_text, format!("{} 1:1", path.display()));
    app.assert_invariants();
}

#[test]
fn status_bar_tracks_soft_wrap_cursor_grid() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "abcdef");

    common::draw(&mut app, window_id, 3, 4);
    common::tick(&mut app, &mut io);
    let status_text = app
        .buffers
        .keys()
        .map(|buffer_id| buffer_id.text(&app).to_string())
        .find(|text| text.starts_with("scratch"))
        .unwrap();

    assert_eq!(status_text, "scratch 2:4");
    app.assert_invariants();
}

#[test]
fn mouse_move_to_status_bar_switches_focus() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "main");
    common::tick(&mut app, &mut io);
    common::draw(&mut app, window_id, 20, 3);

    let cell_h = app.cell_size()[1] as f32;
    common::mouse_moved(&mut app, &mut io, window_id, [0.0, cell_h * 2.5]);
    common::char_input(&mut app, &mut io, window_id, 'X');
    let drawing = common::draw(&mut app, window_id, 20, 3);
    let bottom_row_text: String = drawing
        .commands
        .iter()
        .filter_map(|command| {
            let DrawCommand::Character(c) = command else {
                return None;
            };
            let row = (c.dst.pos[1] / cell_h).round() as usize;
            (row == 2 && c.color == TEXT_COLOR && c.ch != FULL_BLOCK).then_some(c.ch)
        })
        .collect();

    assert_eq!(common::text(&app), "main");
    assert!(bottom_row_text.contains('X'));
    app.assert_invariants();
}

#[test]
fn mouse_move_back_to_editor_restores_focus() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::tick(&mut app, &mut io);
    common::draw(&mut app, window_id, 20, 3);

    let cell_h = app.cell_size()[1] as f32;
    common::mouse_moved(&mut app, &mut io, window_id, [0.0, cell_h * 2.5]);
    common::char_input(&mut app, &mut io, window_id, 'X');
    common::mouse_moved(&mut app, &mut io, window_id, [0.0, cell_h * 0.5]);
    common::char_input(&mut app, &mut io, window_id, 'Y');

    assert_eq!(common::text(&app), "Y");
    app.assert_invariants();
}

#[test]
fn status_bar_focus_loss_autosaves_main_editor() {
    let path = std::path::PathBuf::from("/tmp/focus-status-autosave-test.txt");
    let (mut app, mut io, window_id) = common::file_app(path.clone(), "before");
    common::tick(&mut app, &mut io);

    io.frame_start += std::time::Duration::from_secs(1);
    common::tick(&mut app, &mut io);
    common::alt_key(&mut app, &mut io, window_id, Key::Character("k"));
    common::text_input(&mut app, &mut io, window_id, " after");
    common::draw(&mut app, window_id, 20, 3);

    let cell_h = app.cell_size()[1] as f32;
    common::mouse_moved(&mut app, &mut io, window_id, [0.0, cell_h * 2.5]);

    assert_eq!(io.files.get(&path).unwrap().0, b"before after");
    app.assert_invariants();
}

#[test]
fn status_bar_mouse_coordinates_are_translated() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::tick(&mut app, &mut io);
    common::draw(&mut app, window_id, 20, 3);

    let cell_size = app.cell_size();
    let cell_h = cell_size[1] as f32;
    common::mouse_moved(&mut app, &mut io, window_id, [0.0, cell_h * 2.5]);
    let point = common::point_for_offset(cell_size, 2, 2);
    common::mouse_button(
        &mut app,
        &mut io,
        window_id,
        focus_core::input::ButtonState::Pressed,
        point,
    );
    common::mouse_button(
        &mut app,
        &mut io,
        window_id,
        focus_core::input::ButtonState::Released,
        point,
    );
    common::char_input(&mut app, &mut io, window_id, 'X');
    let drawing = common::draw(&mut app, window_id, 20, 3);
    let bottom_row_text: String = drawing
        .commands
        .iter()
        .filter_map(|command| {
            let DrawCommand::Character(c) = command else {
                return None;
            };
            let row = (c.dst.pos[1] / cell_h).round() as usize;
            (row == 2 && c.color == TEXT_COLOR && c.ch != FULL_BLOCK).then_some(c.ch)
        })
        .collect();

    assert!(bottom_row_text.contains("scXratch"));
    app.assert_invariants();
}

#[test]
fn dragging_over_status_bar_does_not_switch_focus() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "abcd");
    common::draw(&mut app, window_id, 20, 3);

    let cell_size = app.cell_size();
    let start = common::point_for_offset(cell_size, 1, 0);
    let end = [0.0, cell_size[1] as f32 * 2.5];
    common::mouse_button(
        &mut app,
        &mut io,
        window_id,
        focus_core::input::ButtonState::Pressed,
        start,
    );
    common::mouse_moved(&mut app, &mut io, window_id, end);
    common::tick(&mut app, &mut io);
    common::mouse_button(
        &mut app,
        &mut io,
        window_id,
        focus_core::input::ButtonState::Released,
        end,
    );
    common::char_input(&mut app, &mut io, window_id, 'X');

    assert!(common::text(&app).contains('X'));
    app.assert_invariants();
}

#[test]
fn status_bar_cursor_draws_only_when_status_bar_focused() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::tick(&mut app, &mut io);
    let drawing = common::draw(&mut app, window_id, 20, 3);
    let cell_h = app.cell_size()[1] as f32;
    let cursor_rows_before: Vec<usize> = drawing
        .commands
        .iter()
        .filter_map(|command| {
            let DrawCommand::Character(c) = command else {
                return None;
            };
            (c.color == TEXT_COLOR && c.ch == FULL_BLOCK)
                .then_some((c.dst.pos[1] / cell_h).round() as usize)
        })
        .collect();

    common::mouse_moved(&mut app, &mut io, window_id, [0.0, cell_h * 2.5]);
    let drawing = common::draw(&mut app, window_id, 20, 3);
    let cursor_rows_after: Vec<usize> = drawing
        .commands
        .iter()
        .filter_map(|command| {
            let DrawCommand::Character(c) = command else {
                return None;
            };
            (c.color == TEXT_COLOR && c.ch == FULL_BLOCK)
                .then_some((c.dst.pos[1] / cell_h).round() as usize)
        })
        .collect();

    assert!(cursor_rows_before.contains(&0));
    assert!(!cursor_rows_before.contains(&2));
    assert!(!cursor_rows_after.contains(&0));
    assert!(cursor_rows_after.contains(&2));
    app.assert_invariants();
}
