use focus_core::input::{InputEvent, Key};

mod common;

#[test]
fn ctrl_n_opens_a_new_window_with_a_new_document() {
    let (mut app, mut io, window_id) = common::scratch_app();

    common::control_key(&mut app, &mut io, window_id, Key::Character("n"));

    assert_eq!(app.windows.len(), 2);
    assert_eq!(app.editors.len(), 2);
    assert_eq!(app.documents.len(), 2);
    assert_eq!(io.open_windows.len(), 2);
    app.assert_invariants();
}

#[test]
fn ctrl_m_opens_a_new_window_on_the_same_document() {
    let (mut app, mut io, window_id) = common::scratch_app();

    common::control_key(&mut app, &mut io, window_id, Key::Character("m"));

    assert_eq!(app.windows.len(), 2);
    assert_eq!(app.editors.len(), 2);
    assert_eq!(app.documents.len(), 1);
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
