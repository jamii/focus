#![allow(dead_code)]

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use focus_core::app::App;
use focus_core::buffer::{self, BufferId};
use focus_core::drawing::{DrawCommand, Drawing, FULL_BLOCK};
use focus_core::fuzz::MockIO;
use focus_core::input::{ButtonState, InputEvent, Key, ModifiersState, NamedKey};
use focus_core::style::TEXT_COLOR;
use focus_core::window::{self, WindowId};

pub fn scratch_app() -> (App, MockIO, WindowId) {
    let mut io = MockIO::new();
    let mut app = App::new(&mut io);
    let window_id = window::open_scratch(&mut app, &mut io);
    (app, io, window_id)
}

pub fn file_app(path: PathBuf, text: &str) -> (App, MockIO, WindowId) {
    let mut io = MockIO::new();
    let path = if path.is_absolute() {
        path
    } else {
        std::env::current_dir().unwrap().join(path)
    };
    io.files.insert(
        path.clone(),
        (
            text.as_bytes().to_vec(),
            SystemTime::UNIX_EPOCH + Duration::from_secs(1),
        ),
    );
    let mut app = App::new(&mut io);
    let buffer_id = buffer::from_file(&mut app, path);
    let window_id = window::open_edit(&mut app, &mut io, buffer_id);
    (app, io, window_id)
}

pub fn buffer_id(app: &App) -> BufferId {
    app.buffers.keys().min().unwrap()
}

pub fn text(app: &App) -> String {
    buffer_id(app).text(app).to_string()
}

pub fn key(app: &mut App, io: &mut MockIO, window_id: WindowId, key: Key<'_>) {
    app.input(
        io,
        window_id,
        InputEvent::Key {
            state: ButtonState::Pressed,
            logical_key: key,
        },
    );
}

pub fn char_input(app: &mut App, io: &mut MockIO, window_id: WindowId, ch: char) {
    let mut buf = [0u8; 4];
    key(app, io, window_id, Key::Character(ch.encode_utf8(&mut buf)));
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
    app.input(io, window_id, InputEvent::ModifiersChanged(modifiers));
}

pub fn control_key(app: &mut App, io: &mut MockIO, window_id: WindowId, key: Key<'_>) {
    modifiers(
        app,
        io,
        window_id,
        ModifiersState {
            control: true,
            ..Default::default()
        },
    );
    self::key(app, io, window_id, key);
    modifiers(app, io, window_id, ModifiersState::default());
}

pub fn control_shift_key(app: &mut App, io: &mut MockIO, window_id: WindowId, key: Key<'_>) {
    modifiers(
        app,
        io,
        window_id,
        ModifiersState {
            control: true,
            shift: true,
            ..Default::default()
        },
    );
    self::key(app, io, window_id, key);
    modifiers(app, io, window_id, ModifiersState::default());
}

pub fn alt_key(app: &mut App, io: &mut MockIO, window_id: WindowId, key: Key<'_>) {
    modifiers(
        app,
        io,
        window_id,
        ModifiersState {
            alt: true,
            ..Default::default()
        },
    );
    self::key(app, io, window_id, key);
    modifiers(app, io, window_id, ModifiersState::default());
}

pub fn mouse_button(
    app: &mut App,
    io: &mut MockIO,
    window_id: WindowId,
    state: ButtonState,
    position: [f32; 2],
) {
    io.mouse_position = position;
    app.input(io, window_id, InputEvent::MouseButton { state, position });
}

pub fn mouse_moved(app: &mut App, io: &mut MockIO, window_id: WindowId, position: [f32; 2]) {
    io.mouse_position = position;
    // A newly mapped Wayland window receives a synthetic CursorMoved before
    // real motion. Model that initial event before the deliberate movement.
    app.input(io, window_id, InputEvent::MouseMoved { position });
    app.input(io, window_id, InputEvent::MouseMoved { position });
}

pub fn mouse_wheel(app: &mut App, io: &mut MockIO, window_id: WindowId, y_offset: f32) {
    app.input(io, window_id, InputEvent::MouseWheel { y_offset });
}

pub fn focus_changed(app: &mut App, io: &mut MockIO, window_id: WindowId, focused: bool) {
    app.input(io, window_id, InputEvent::FocusChanged { focused });
}

pub fn tick(app: &mut App, io: &mut MockIO) {
    let frame_start = io.frame_start;
    app.tick(io, frame_start);
}

pub fn point_for_offset(cell_size: [u32; 2], offset: usize, line: usize) -> [f32; 2] {
    let [cell_w, cell_h] = [cell_size[0] as f32, cell_size[1] as f32];
    [
        cell_w * (offset + 1) as f32,
        cell_h * line as f32 + cell_h / 2.0,
    ]
}

// A second window on the same buffer, with its cursor at the start.
// Ctrl+n copies the page, cursors included, so callers that position two
// cursors against each other would otherwise start from wherever the
// original happened to be.
pub fn open_same_buffer_window(app: &mut App, io: &mut MockIO, window_id: WindowId) -> WindowId {
    let before = io.open_windows.clone();
    control_key(app, io, window_id, Key::Character("n"));
    let window_id_new = *io
        .open_windows
        .iter()
        .find(|window_id| !before.contains(window_id))
        .unwrap();
    alt_key(app, io, window_id_new, Key::Character("i"));
    window_id_new
}

pub fn draw(app: &mut App, window_id: WindowId, wrap_chars: usize, rows: usize) -> Drawing {
    let [cell_w, cell_h] = [app.cell_size()[0] as f32, app.cell_size()[1] as f32];
    let mut drawing = Drawing::new([cell_w * (wrap_chars + 2) as f32, cell_h * rows as f32]);
    app.draw(window_id, &mut drawing);
    drawing
}

pub fn text_line_lengths(cell_size: [u32; 2], drawing: &Drawing) -> Vec<usize> {
    let cell_h = cell_size[1] as f32;
    let mut lines = Vec::new();
    for command in &drawing.commands {
        let DrawCommand::Character(c) = command else {
            continue;
        };
        // Text glyphs: text color, and not the Full Block used for fills.
        if c.color != TEXT_COLOR || c.ch == FULL_BLOCK {
            continue;
        }
        let row = (c.dst.pos[1] / cell_h).round() as usize;
        if lines.len() <= row {
            lines.resize(row + 1, 0);
        }
        lines[row] += 1;
    }
    lines
}

pub fn cursor_lines(cell_size: [u32; 2], drawing: &Drawing) -> Vec<usize> {
    let cell_h = cell_size[1] as f32;
    let bottom_row = drawing
        .commands
        .iter()
        .filter_map(|command| {
            let DrawCommand::Character(c) = command else {
                return None;
            };
            Some(((c.dst.pos[1] + c.dst.size[1] - 1.0) / cell_h).floor() as usize)
        })
        .max();
    drawing
        .commands
        .iter()
        .filter_map(|command| {
            let DrawCommand::Character(c) = command else {
                return None;
            };
            // Cursors are Full Block fills in the text color.
            let row = (c.dst.pos[1] / cell_h).round() as usize;
            if c.color == TEXT_COLOR && c.ch == FULL_BLOCK && Some(row) != bottom_row {
                Some((c.dst.pos[1] / cell_h).round() as usize)
            } else {
                None
            }
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
