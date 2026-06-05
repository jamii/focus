#![allow(dead_code)]

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use focus::app::{App, DocumentId, InputEvent, WindowId};
use focus::drawing::{DrawCommand, Drawing};
use focus::fuzz::MockIO;
use focus::style::TEXT_COLOR;
use winit::event::ElementState;
use winit::keyboard::{Key, ModifiersState, NamedKey};

pub fn sync_app_io(app: &mut App, io: &MockIO) {
    focus::fuzz::sync_app_io(app, io);
}

pub fn scratch_app() -> (App, MockIO, WindowId) {
    let mut io = MockIO::new();
    let window_id = io.fresh_window_id();
    io.open_windows.push(window_id);
    let app = App::new(window_id, &mut io, None);
    (app, io, window_id)
}

pub fn file_app(path: PathBuf, text: &str) -> (App, MockIO, WindowId) {
    let mut io = MockIO::new();
    io.files.insert(
        path.clone(),
        (
            text.as_bytes().to_vec(),
            SystemTime::UNIX_EPOCH + Duration::from_secs(1),
        ),
    );
    let window_id = io.fresh_window_id();
    io.open_windows.push(window_id);
    let app = App::new(window_id, &mut io, Some(path));
    (app, io, window_id)
}

pub fn document_id(app: &App) -> DocumentId {
    *app.documents.keys().next().unwrap()
}

pub fn text(app: &App) -> String {
    let document = document_id(app).get(app);
    String::from_utf8_lossy(&document.text).to_string()
}

pub fn key(app: &mut App, io: &mut MockIO, window_id: WindowId, key: Key) {
    sync_app_io(app, io);
    app.input(
        io,
        window_id,
        InputEvent::Key {
            state: ElementState::Pressed,
            logical_key: key,
        },
    );
}

pub fn char_input(app: &mut App, io: &mut MockIO, window_id: WindowId, ch: char) {
    key(app, io, window_id, Key::Character(ch.to_string().into()));
}

pub fn text_input(app: &mut App, io: &mut MockIO, window_id: WindowId, text: &str) {
    for ch in text.chars() {
        match ch {
            '\n' => key(app, io, window_id, Key::Named(NamedKey::Enter)),
            ' ' => key(app, io, window_id, Key::Named(NamedKey::Space)),
            ch => char_input(app, io, window_id, ch),
        }
    }
}

pub fn modifiers(app: &mut App, io: &mut MockIO, window_id: WindowId, modifiers: ModifiersState) {
    sync_app_io(app, io);
    app.input(io, window_id, InputEvent::ModifiersChanged(modifiers));
}

pub fn control_key(app: &mut App, io: &mut MockIO, window_id: WindowId, key: Key) {
    modifiers(app, io, window_id, ModifiersState::CONTROL);
    self::key(app, io, window_id, key);
    modifiers(app, io, window_id, ModifiersState::empty());
}

pub fn alt_key(app: &mut App, io: &mut MockIO, window_id: WindowId, key: Key) {
    modifiers(app, io, window_id, ModifiersState::ALT);
    self::key(app, io, window_id, key);
    modifiers(app, io, window_id, ModifiersState::empty());
}

pub fn mouse_button(
    app: &mut App,
    io: &mut MockIO,
    window_id: WindowId,
    state: ElementState,
    position: [f32; 2],
) {
    io.mouse_pos = position;
    sync_app_io(app, io);
    app.input(io, window_id, InputEvent::MouseButton { state, position });
}

pub fn mouse_wheel(app: &mut App, io: &mut MockIO, window_id: WindowId, y_offset: f32) {
    sync_app_io(app, io);
    app.input(io, window_id, InputEvent::MouseWheel { y_offset });
}

pub fn focus_changed(app: &mut App, io: &mut MockIO, window_id: WindowId, focused: bool) {
    sync_app_io(app, io);
    app.input(io, window_id, InputEvent::FocusChanged { focused });
}

pub fn tick(app: &mut App, io: &mut MockIO) {
    sync_app_io(app, io);
    app.tick(io);
}

pub fn point_for_offset(app: &App, offset: usize, line: usize) -> [f32; 2] {
    let cell_w = app.atlas.cell_size[0] as f32;
    let cell_h = app.atlas.cell_size[1] as f32;
    [
        cell_w * (offset + 1) as f32,
        cell_h * line as f32 + cell_h / 2.0,
    ]
}

pub fn open_same_document_window(app: &mut App, io: &mut MockIO, window_id: WindowId) -> WindowId {
    let before = io.open_windows.clone();
    control_key(app, io, window_id, Key::Character("m".into()));
    *io.open_windows
        .iter()
        .find(|window_id| !before.contains(window_id))
        .unwrap()
}

pub fn draw(app: &mut App, window_id: WindowId, wrap_chars: usize, rows: usize) -> Drawing {
    let cell_w = app.atlas.cell_size[0] as f32;
    let cell_h = app.atlas.cell_size[1] as f32;
    let mut drawing = Drawing::new([cell_w * (wrap_chars + 2) as f32, cell_h * rows as f32]);
    app.draw(window_id, &mut drawing);
    drawing
}

pub fn text_line_lengths(app: &App, drawing: &Drawing) -> Vec<usize> {
    let mut lines = Vec::new();
    let cell_h = app.atlas.cell_size[1] as f32;
    for command in &drawing.commands {
        let DrawCommand::Quad(quad) = command else {
            continue;
        };
        if quad.color != TEXT_COLOR || quad.src_size != app.atlas.cell_size {
            continue;
        }
        if quad.src_pos == app.atlas.white_pos {
            continue;
        }
        let row = (quad.dst_pos[1] / cell_h).round() as usize;
        if lines.len() <= row {
            lines.resize(row + 1, 0);
        }
        lines[row] += 1;
    }
    lines
}

pub fn cursor_lines(app: &App, drawing: &Drawing) -> Vec<usize> {
    let cell_h = app.atlas.cell_size[1] as f32;
    drawing
        .commands
        .iter()
        .filter_map(|command| {
            let DrawCommand::Quad(quad) = command else {
                return None;
            };
            if quad.color == TEXT_COLOR && quad.src_pos == app.atlas.white_pos {
                Some((quad.dst_pos[1] / cell_h).round() as usize)
            } else {
                None
            }
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
