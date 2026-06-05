use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use focus::app::InputEvent;
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

    common::sync_app_io(&mut app, &io);
    app.tick(&mut io);

    assert_eq!(common::text(&app), "from disk\n");
    app.assert_invariants();
}

#[test]
fn tick_reloads_clean_file_after_external_change() {
    let path = PathBuf::from("/tmp/focus-document-reload-test.txt");
    let (mut app, mut io, _) = common::file_app(path.clone(), "before\n");
    common::sync_app_io(&mut app, &io);
    app.tick(&mut io);

    io.frame_start += Duration::from_secs(1);
    io.files.insert(
        path,
        (
            b"after\n".to_vec(),
            SystemTime::UNIX_EPOCH + Duration::from_secs(2),
        ),
    );
    common::sync_app_io(&mut app, &io);
    app.tick(&mut io);

    assert_eq!(common::text(&app), "after\n");
    app.assert_invariants();
}

#[test]
fn explicit_save_writes_modified_file() {
    let path = PathBuf::from("/tmp/focus-document-explicit-save-test.txt");
    let (mut app, mut io, window_id) = common::file_app(path.clone(), "before");
    common::tick(&mut app, &mut io);

    io.frame_start += Duration::from_secs(1);
    common::text_input(&mut app, &mut io, window_id, " after");
    common::control_key(&mut app, &mut io, window_id, Key::Character("s".into()));

    assert_eq!(io.files.get(&path).unwrap().0, b"before after");
    app.assert_invariants();
}

#[test]
fn focus_loss_autosaves_existing_modified_file() {
    let path = PathBuf::from("/tmp/focus-document-autosave-test.txt");
    let (mut app, mut io, window_id) = common::file_app(path.clone(), "before");
    common::tick(&mut app, &mut io);

    io.frame_start += Duration::from_secs(1);
    common::text_input(&mut app, &mut io, window_id, " after");
    common::focus_changed(&mut app, &mut io, window_id, false);

    assert_eq!(io.files.get(&path).unwrap().0, b"before after");
    app.assert_invariants();
}

#[test]
fn autosave_does_not_recreate_deleted_file_but_explicit_save_does() {
    let path = PathBuf::from("/tmp/focus-document-deleted-save-test.txt");
    let (mut app, mut io, window_id) = common::file_app(path.clone(), "before");
    common::tick(&mut app, &mut io);

    io.files.remove(&path);
    io.frame_start += Duration::from_secs(1);
    common::text_input(&mut app, &mut io, window_id, " after");
    common::focus_changed(&mut app, &mut io, window_id, false);
    assert!(!io.files.contains_key(&path));

    common::control_key(&mut app, &mut io, window_id, Key::Character("s".into()));
    assert_eq!(io.files.get(&path).unwrap().0, b"before after");
    app.assert_invariants();
}

#[test]
fn saving_scratch_document_is_a_noop() {
    let (mut app, mut io, window_id) = common::scratch_app();

    common::text_input(&mut app, &mut io, window_id, "scratch");
    common::control_key(&mut app, &mut io, window_id, Key::Character("s".into()));

    assert!(io.files.is_empty());
    app.assert_invariants();
}

#[test]
fn dirty_file_document_does_not_reload_external_changes() {
    let path = PathBuf::from("/tmp/focus-document-dirty-reload-test.txt");
    let (mut app, mut io, window_id) = common::file_app(path.clone(), "before");
    common::tick(&mut app, &mut io);

    io.frame_start += Duration::from_secs(1);
    common::text_input(&mut app, &mut io, window_id, " local");
    io.files.insert(
        path,
        (
            b"external".to_vec(),
            SystemTime::UNIX_EPOCH + Duration::from_secs(3),
        ),
    );
    io.frame_start += Duration::from_secs(1);
    common::tick(&mut app, &mut io);

    assert_eq!(common::text(&app), "before local");
    app.assert_invariants();
}

#[test]
fn clean_external_replacement_updates_text_and_cursor_offsets() {
    let path = PathBuf::from("/tmp/focus-document-replacement-reload-test.txt");
    let (mut app, mut io, window_id) = common::file_app(path.clone(), "abcdef");
    common::tick(&mut app, &mut io);

    common::control_key(&mut app, &mut io, window_id, Key::Character("j".into()));
    io.files.insert(
        path,
        (
            b"aXYef".to_vec(),
            SystemTime::UNIX_EPOCH + Duration::from_secs(2),
        ),
    );
    io.frame_start += Duration::from_secs(1);
    common::tick(&mut app, &mut io);
    common::char_input(&mut app, &mut io, window_id, 'Q');

    assert_eq!(common::text(&app), "aXYeQf");
    app.assert_invariants();
}

#[test]
fn ignored_focus_gain_does_not_autosave() {
    let path = PathBuf::from("/tmp/focus-document-focus-gain-test.txt");
    let (mut app, mut io, window_id) = common::file_app(path.clone(), "before");
    common::tick(&mut app, &mut io);

    io.frame_start += Duration::from_secs(1);
    common::text_input(&mut app, &mut io, window_id, " after");
    app.input(&mut io, window_id, InputEvent::FocusChanged { focused: true });

    assert_eq!(io.files.get(&path).unwrap().0, b"before");
    app.assert_invariants();
}
