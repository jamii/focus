use focus_core::drawing::{DrawCommand, FULL_BLOCK};
use focus_core::input::{InputEvent, Key};
use focus_core::style::TEXT_COLOR;

mod common;

#[test]
fn ctrl_n_opens_a_new_window_with_a_new_document() {
    let (mut app, mut io, window_id) = common::scratch_app();

    common::control_key(&mut app, &mut io, window_id, Key::Character("n"));

    assert_eq!(app.windows.len(), 2);
    assert_eq!(app.pages.len(), 2);
    assert_eq!(app.editors.len(), 4);
    assert_eq!(app.documents.len(), 4);
    assert_eq!(io.open_windows.len(), 2);
    app.assert_invariants();
}

#[test]
fn ctrl_m_opens_a_new_window_on_the_same_document() {
    let (mut app, mut io, window_id) = common::scratch_app();

    common::control_key(&mut app, &mut io, window_id, Key::Character("m"));

    assert_eq!(app.windows.len(), 2);
    assert_eq!(app.pages.len(), 2);
    assert_eq!(app.editors.len(), 4);
    assert_eq!(app.documents.len(), 3);
    assert_eq!(io.open_windows.len(), 2);
    app.assert_invariants();
}

#[test]
fn closing_last_window_exits() {
    let (mut app, mut io, window_id) = common::scratch_app();

    app.input(&mut io, window_id, InputEvent::CloseRequested);

    assert!(app.windows.is_empty());
    assert!(io.open_windows.is_empty());
    assert!(io.exited);
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
