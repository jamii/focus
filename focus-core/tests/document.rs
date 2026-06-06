use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use bstr::BString;
use focus_core::app::{App, IO, WindowSize};
use focus_core::input::{ElementState, InputEvent, Key, ModifiersState, NamedKey};
use focus_core::window::WindowId;

mod common;

struct ErrorIO {
    inner: focus_core::fuzz::MockIO,
    file_mtime_error: Option<std::io::ErrorKind>,
    file_read_error: Option<std::io::ErrorKind>,
    file_write_error: Option<std::io::ErrorKind>,
}

impl ErrorIO {
    fn new() -> Self {
        Self {
            inner: focus_core::fuzz::MockIO::new(),
            file_mtime_error: None,
            file_read_error: None,
            file_write_error: None,
        }
    }

    fn sync_app_io(&self, app: &mut App) {
        app.frame_start = self.inner.frame_start;
        app.mouse_position = self.inner.mouse_pos;
    }
}

impl IO for ErrorIO {
    fn open_window(&mut self, title: String, size: WindowSize) -> WindowId {
        self.inner.open_window(title, size)
    }

    fn close_window(&mut self, window_id: WindowId) {
        self.inner.close_window(window_id);
    }

    fn set_window_title(&mut self, window_id: WindowId, title: String) {
        self.inner.set_window_title(window_id, title);
    }

    fn request_redraw(&mut self, window_id: WindowId) {
        self.inner.request_redraw(window_id);
    }

    fn rebuild_atlas(&mut self, px_size: f32) -> [u32; 2] {
        self.inner.rebuild_atlas(px_size)
    }

    fn get_clipboard_text(&mut self) -> Option<BString> {
        self.inner.get_clipboard_text()
    }

    fn set_clipboard_text(&mut self, text: BString) {
        self.inner.set_clipboard_text(text);
    }

    fn exit(&mut self) {
        self.inner.exit();
    }

    fn file_mtime(&mut self, path: &Path) -> std::io::Result<SystemTime> {
        if let Some(kind) = self.file_mtime_error {
            return Err(std::io::Error::from(kind));
        }
        self.inner.file_mtime(path)
    }

    fn file_read(&mut self, path: &Path) -> std::io::Result<Vec<u8>> {
        if let Some(kind) = self.file_read_error {
            return Err(std::io::Error::from(kind));
        }
        self.inner.file_read(path)
    }

    fn file_write(
        &mut self,
        path: &Path,
        contents: &[u8],
        create: bool,
    ) -> std::io::Result<SystemTime> {
        if let Some(kind) = self.file_write_error {
            return Err(std::io::Error::from(kind));
        }
        self.inner.file_write(path, contents, create)
    }
}

fn error_file_app(path: PathBuf, text: &str) -> (App, ErrorIO, WindowId) {
    let mut io = ErrorIO::new();
    io.inner.files.insert(
        path.clone(),
        (
            text.as_bytes().to_vec(),
            SystemTime::UNIX_EPOCH + Duration::from_secs(1),
        ),
    );
    let window_id = io.inner.fresh_window_id();
    io.inner.open_windows.push(window_id);
    let app = App::new(window_id, &mut io, Some(path));
    (app, io, window_id)
}

fn error_key(app: &mut App, io: &mut ErrorIO, window_id: WindowId, key: Key<'_>) {
    io.sync_app_io(app);
    app.input(
        io,
        window_id,
        InputEvent::Key {
            state: ElementState::Pressed,
            logical_key: key,
        },
    );
}

fn error_text_input(app: &mut App, io: &mut ErrorIO, window_id: WindowId, text: &str) {
    for ch in text.chars() {
        match ch {
            '\n' => error_key(app, io, window_id, Key::Named(NamedKey::Enter)),
            ' ' => error_key(app, io, window_id, Key::Named(NamedKey::Space)),
            ch => {
                let mut buf = [0u8; 4];
                error_key(app, io, window_id, Key::Character(ch.encode_utf8(&mut buf)))
            }
        }
    }
}

fn error_control_key(app: &mut App, io: &mut ErrorIO, window_id: WindowId, key: Key<'_>) {
    io.sync_app_io(app);
    app.input(
        io,
        window_id,
        InputEvent::ModifiersChanged(ModifiersState {
            control: true,
            ..Default::default()
        }),
    );
    error_key(app, io, window_id, key);
    io.sync_app_io(app);
    app.input(
        io,
        window_id,
        InputEvent::ModifiersChanged(ModifiersState::default()),
    );
}

fn error_focus_changed(app: &mut App, io: &mut ErrorIO, window_id: WindowId, focused: bool) {
    io.sync_app_io(app);
    app.input(io, window_id, InputEvent::FocusChanged { focused });
}

fn error_tick(app: &mut App, io: &mut ErrorIO) {
    io.sync_app_io(app);
    app.tick(io);
}

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

    common::control_key(&mut app, &mut io, window_id, Key::Character("j"));
    common::control_key(&mut app, &mut io, window_id, Key::Character("j"));
    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Space));
    common::control_key(&mut app, &mut io, window_id, Key::Character("j"));
    common::control_key(&mut app, &mut io, window_id, Key::Character("j"));
    common::char_input(&mut app, &mut io, window_id, 'X');

    assert_eq!(common::text(&app), "hXlo");
    app.assert_invariants();
}

#[test]
fn undo_and_redo_restore_text() {
    let (mut app, mut io, window_id) = common::scratch_app();
    common::text_input(&mut app, &mut io, window_id, "abc");

    common::control_key(&mut app, &mut io, window_id, Key::Character("z"));
    assert_eq!(common::text(&app), "");

    common::control_key(&mut app, &mut io, window_id, Key::Character("Z"));
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
    common::control_key(&mut app, &mut io, window_id, Key::Character("s"));

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

    common::control_key(&mut app, &mut io, window_id, Key::Character("s"));
    assert_eq!(io.files.get(&path).unwrap().0, b"before after");
    app.assert_invariants();
}

#[test]
fn saving_scratch_document_is_a_noop() {
    let (mut app, mut io, window_id) = common::scratch_app();

    common::text_input(&mut app, &mut io, window_id, "scratch");
    common::control_key(&mut app, &mut io, window_id, Key::Character("s"));

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
    let (mut app, mut io, window_id) = common::file_app(path.clone(), "abcd ef");
    common::tick(&mut app, &mut io);

    common::control_key(&mut app, &mut io, window_id, Key::Character("j"));
    io.files.insert(
        path,
        (
            b"aXY ef".to_vec(),
            SystemTime::UNIX_EPOCH + Duration::from_secs(2),
        ),
    );
    io.frame_start += Duration::from_secs(1);
    common::tick(&mut app, &mut io);
    common::char_input(&mut app, &mut io, window_id, 'Q');

    assert_eq!(common::text(&app), "aXY eQf");
    app.assert_invariants();
}

#[test]
fn ignored_focus_gain_does_not_autosave() {
    let path = PathBuf::from("/tmp/focus-document-focus-gain-test.txt");
    let (mut app, mut io, window_id) = common::file_app(path.clone(), "before");
    common::tick(&mut app, &mut io);

    io.frame_start += Duration::from_secs(1);
    common::text_input(&mut app, &mut io, window_id, " after");
    app.input(
        &mut io,
        window_id,
        InputEvent::FocusChanged { focused: true },
    );

    assert_eq!(io.files.get(&path).unwrap().0, b"before");
    app.assert_invariants();
}

#[test]
fn undo_redo_batching_and_redo_clearing_follow_input_flushes() {
    let (mut app, mut io, window_id) = common::scratch_app();

    common::text_input(&mut app, &mut io, window_id, "ab");
    common::control_key(&mut app, &mut io, window_id, Key::Character("z"));
    assert_eq!(common::text(&app), "");

    common::control_key(&mut app, &mut io, window_id, Key::Character("Z"));
    assert_eq!(common::text(&app), "ab");

    common::char_input(&mut app, &mut io, window_id, 'c');
    common::control_key(&mut app, &mut io, window_id, Key::Character("Z"));

    assert_eq!(common::text(&app), "abc");
    app.assert_invariants();
}

#[test]
fn tick_flushes_idle_edit_batch_for_undo() {
    let (mut app, mut io, window_id) = common::scratch_app();

    common::text_input(&mut app, &mut io, window_id, "a");
    io.frame_start += Duration::from_millis(1500);
    common::tick(&mut app, &mut io);
    common::text_input(&mut app, &mut io, window_id, "b");
    common::control_key(&mut app, &mut io, window_id, Key::Character("z"));

    assert_eq!(common::text(&app), "a");
    app.assert_invariants();
}

#[test]
fn explicit_save_error_leaves_file_dirty_until_next_successful_save() {
    let path = PathBuf::from("/tmp/focus-document-explicit-save-error-test.txt");
    let (mut app, mut io, window_id) = error_file_app(path.clone(), "before");
    error_tick(&mut app, &mut io);

    io.inner.frame_start += Duration::from_secs(1);
    error_text_input(&mut app, &mut io, window_id, " after");
    io.file_write_error = Some(std::io::ErrorKind::PermissionDenied);
    error_control_key(&mut app, &mut io, window_id, Key::Character("s"));
    assert_eq!(io.inner.files.get(&path).unwrap().0, b"before");

    io.file_write_error = None;
    io.inner.frame_start += Duration::from_secs(1);
    error_control_key(&mut app, &mut io, window_id, Key::Character("s"));
    assert_eq!(io.inner.files.get(&path).unwrap().0, b"before after");
    app.assert_invariants();
}

#[test]
fn autosave_non_notfound_error_leaves_file_dirty_until_explicit_save() {
    let path = PathBuf::from("/tmp/focus-document-autosave-error-test.txt");
    let (mut app, mut io, window_id) = error_file_app(path.clone(), "before");
    error_tick(&mut app, &mut io);

    io.inner.frame_start += Duration::from_secs(1);
    error_text_input(&mut app, &mut io, window_id, " after");
    io.file_write_error = Some(std::io::ErrorKind::PermissionDenied);
    error_focus_changed(&mut app, &mut io, window_id, false);
    assert_eq!(io.inner.files.get(&path).unwrap().0, b"before");

    io.file_write_error = None;
    io.inner.frame_start += Duration::from_secs(1);
    error_control_key(&mut app, &mut io, window_id, Key::Character("s"));
    assert_eq!(io.inner.files.get(&path).unwrap().0, b"before after");
    app.assert_invariants();
}

#[test]
fn reload_mtime_error_keeps_clean_document_unchanged() {
    let path = PathBuf::from("/tmp/focus-document-mtime-error-test.txt");
    let (mut app, mut io, _) = error_file_app(path.clone(), "before");
    error_tick(&mut app, &mut io);

    io.inner.files.insert(
        path,
        (
            b"after".to_vec(),
            SystemTime::UNIX_EPOCH + Duration::from_secs(2),
        ),
    );
    io.file_mtime_error = Some(std::io::ErrorKind::PermissionDenied);
    io.inner.frame_start += Duration::from_secs(1);
    error_tick(&mut app, &mut io);

    assert_eq!(common::text(&app), "before");
    app.assert_invariants();
}

#[test]
fn reload_read_error_keeps_old_text_and_retries_later() {
    let path = PathBuf::from("/tmp/focus-document-read-error-test.txt");
    let (mut app, mut io, _) = error_file_app(path.clone(), "before");
    error_tick(&mut app, &mut io);

    io.inner.files.insert(
        path,
        (
            b"after".to_vec(),
            SystemTime::UNIX_EPOCH + Duration::from_secs(2),
        ),
    );
    io.file_read_error = Some(std::io::ErrorKind::PermissionDenied);
    io.inner.frame_start += Duration::from_secs(1);
    error_tick(&mut app, &mut io);
    assert_eq!(common::text(&app), "before");

    io.file_read_error = None;
    io.inner.frame_start += Duration::from_secs(1);
    error_tick(&mut app, &mut io);
    assert_eq!(common::text(&app), "after");
    app.assert_invariants();
}

#[test]
fn explicit_save_clean_file_is_a_noop() {
    let path = PathBuf::from("/tmp/focus-document-clean-save-noop-test.txt");
    let (mut app, mut io, window_id) = common::file_app(path.clone(), "before");
    common::tick(&mut app, &mut io);
    let before = io.files.get(&path).unwrap().clone();

    io.frame_start += Duration::from_secs(1);
    common::control_key(&mut app, &mut io, window_id, Key::Character("s"));

    assert_eq!(io.files.get(&path).unwrap(), &before);
    app.assert_invariants();
}

#[test]
fn reload_skips_file_when_mtime_has_not_advanced() {
    let path = PathBuf::from("/tmp/focus-document-same-mtime-reload-test.txt");
    let (mut app, mut io, _) = common::file_app(path.clone(), "before");
    common::tick(&mut app, &mut io);

    io.files.insert(
        path,
        (
            b"after".to_vec(),
            SystemTime::UNIX_EPOCH + Duration::from_secs(1),
        ),
    );
    io.frame_start += Duration::from_secs(1);
    common::tick(&mut app, &mut io);

    assert_eq!(common::text(&app), "before");
    app.assert_invariants();
}

#[test]
fn clean_external_insert_reloads_text() {
    let path = PathBuf::from("/tmp/focus-document-insert-reload-test.txt");
    let (mut app, mut io, _) = common::file_app(path.clone(), "abef");
    common::tick(&mut app, &mut io);

    io.files.insert(
        path,
        (
            b"abcdef".to_vec(),
            SystemTime::UNIX_EPOCH + Duration::from_secs(2),
        ),
    );
    io.frame_start += Duration::from_secs(1);
    common::tick(&mut app, &mut io);

    assert_eq!(common::text(&app), "abcdef");
    app.assert_invariants();
}

#[test]
fn clean_external_delete_reloads_text() {
    let path = PathBuf::from("/tmp/focus-document-delete-reload-test.txt");
    let (mut app, mut io, _) = common::file_app(path.clone(), "abcdef");
    common::tick(&mut app, &mut io);

    io.files.insert(
        path,
        (
            b"abef".to_vec(),
            SystemTime::UNIX_EPOCH + Duration::from_secs(2),
        ),
    );
    io.frame_start += Duration::from_secs(1);
    common::tick(&mut app, &mut io);

    assert_eq!(common::text(&app), "abef");
    app.assert_invariants();
}

#[test]
fn clean_external_multi_hunk_change_reloads_text() {
    let path = PathBuf::from("/tmp/focus-document-multi-hunk-reload-test.txt");
    let (mut app, mut io, _) = common::file_app(path.clone(), "abcdefghi");
    common::tick(&mut app, &mut io);

    io.files.insert(
        path,
        (
            b"aXcdefYhi".to_vec(),
            SystemTime::UNIX_EPOCH + Duration::from_secs(2),
        ),
    );
    io.frame_start += Duration::from_secs(1);
    common::tick(&mut app, &mut io);

    assert_eq!(common::text(&app), "aXcdefYhi");
    app.assert_invariants();
}

#[test]
fn undo_and_redo_empty_stacks_are_noops() {
    let (mut app, mut io, window_id) = common::scratch_app();

    common::control_key(&mut app, &mut io, window_id, Key::Character("z"));
    common::control_key(&mut app, &mut io, window_id, Key::Character("Z"));
    common::char_input(&mut app, &mut io, window_id, 'X');

    assert_eq!(common::text(&app), "X");
    app.assert_invariants();
}
