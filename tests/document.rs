use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use winit::keyboard::{Key, NamedKey};

mod common;

#[test]
fn text_input_builds_scratch_document() {
    let (mut app, mut io, window_id) = common::scratch_app();

    common::text_input(&mut app, &mut io, window_id, "hello\nworld");

    assert_eq!(common::text(&app), "hello\nworld");
    app.assert_invariants();
}

#[test]
fn backspace_updates_document_through_editor() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "abcd");

    common::key(
        &mut app,
        &mut io,
        window_id,
        Key::Named(NamedKey::Backspace),
    );
    common::key(
        &mut app,
        &mut io,
        window_id,
        Key::Named(NamedKey::Backspace),
    );

    assert_eq!(common::text(&app), "ab");
    app.assert_invariants();
}

#[test]
fn selection_replacement_updates_document() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "hello");

    common::control_key(&mut app, &mut io, window_id, Key::Character("j".into()));
    common::control_key(&mut app, &mut io, window_id, Key::Character("j".into()));
    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Space));
    common::control_key(&mut app, &mut io, window_id, Key::Character("j".into()));
    common::control_key(&mut app, &mut io, window_id, Key::Character("j".into()));
    common::char_input(&mut app, &mut io, window_id, 'X');

    assert_eq!(common::text(&app), "hXlo");
    app.assert_invariants();
}

#[test]
fn undo_and_redo_restore_text() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "abc");

    common::control_key(&mut app, &mut io, window_id, Key::Character("z".into()));
    assert_eq!(common::text(&app), "");

    common::control_key(&mut app, &mut io, window_id, Key::Character("Z".into()));
    assert_eq!(common::text(&app), "abc");

    app.assert_invariants();
}

#[test]
fn tick_loads_file_document_from_mock_io() {
    let path = PathBuf::from("/tmp/focus-document-test.txt");
    let (mut app, mut io, _) = common::file_app(path, "from disk\n");

    app.tick(&mut io);

    assert_eq!(common::text(&app), "from disk\n");
    app.assert_invariants();
}

#[test]
fn tick_reloads_clean_file_after_external_change() {
    let path = PathBuf::from("/tmp/focus-document-reload-test.txt");
    let (mut app, mut io, _) = common::file_app(path.clone(), "before\n");
    app.tick(&mut io);

    io.frame_start += Duration::from_secs(1);
    io.files.insert(
        path,
        (
            b"after\n".to_vec(),
            SystemTime::UNIX_EPOCH + Duration::from_secs(2),
        ),
    );
    app.tick(&mut io);

    assert_eq!(common::text(&app), "after\n");
    app.assert_invariants();
}
