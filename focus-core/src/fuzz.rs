// Fuzz harness for the editor.
//
// `fuzz_one` drives an empty `App` with a sequence of synthesized
// `InputEvent`s and ticks, both pulled from a `Frng` (see fuzz_gen.rs).
// Time advances by a fuzzer-chosen delta between events. The IO is mocked
// so the harness runs anywhere.
//
// This is the body of the honggfuzz target (see hfuzz/src/main.rs).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use bstr::BString;

use crate::app::{App, IO, InputEvent, WindowId, WindowSize};
use crate::drawing::Drawing;
use crate::fuzz_gen::Frng;
use crate::input::{ElementState, Key, ModifiersState, NamedKey};

// Mock IO: tracks open windows, fabricates fresh WindowIds, advances
// `frame_start` by whatever the harness pushes via `advance`.
pub struct MockIO {
    next_window_id: usize,
    pub open_windows: Vec<WindowId>,
    pub exited: bool,
    pub screen_size: [f32; 2],
    pub frame_start: Duration,
    pub mouse_pos: [f32; 2],
    pub clipboard: Option<BString>,
    pub files: HashMap<PathBuf, (Vec<u8>, SystemTime)>,
    pub system_time: SystemTime,
}

impl MockIO {
    pub fn new() -> Self {
        MockIO {
            // Start at 1 so a fabricated id is never zero.
            next_window_id: 1,
            open_windows: Vec::new(),
            exited: false,
            screen_size: [0.0, 0.0],
            frame_start: Duration::ZERO,
            mouse_pos: [0.0, 0.0],
            clipboard: None,
            files: HashMap::new(),
            system_time: SystemTime::UNIX_EPOCH,
        }
    }

    pub fn fresh_window_id(&mut self) -> WindowId {
        let id = WindowId(self.next_window_id);
        self.next_window_id += 1;
        id
    }
}

pub fn sync_app_io(app: &mut App, io: &MockIO) {
    app.frame_start = io.frame_start;
    app.mouse_position = io.mouse_pos;
}

impl IO for MockIO {
    fn open_window(&mut self, _title: String, _size: WindowSize) -> WindowId {
        let id = self.fresh_window_id();
        self.open_windows.push(id);
        id
    }

    fn close_window(&mut self, window_id: WindowId) {
        self.open_windows.retain(|w| *w != window_id);
    }

    fn set_window_title(&mut self, _window_id: WindowId, _title: String) {}

    fn get_clipboard_text(&mut self) -> Option<BString> {
        self.clipboard.clone()
    }

    fn set_clipboard_text(&mut self, text: BString) {
        self.clipboard = Some(text);
    }

    fn request_redraw(&mut self, _window_id: WindowId) {}

    fn reload_atlas(&mut self, _pixels: &[u8], _size: [u32; 2]) {}

    fn exit(&mut self) {
        self.exited = true;
    }

    fn file_mtime(&mut self, path: &Path) -> std::io::Result<SystemTime> {
        self.files
            .get(path)
            .map(|(_, m)| *m)
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))
    }

    fn file_read(&mut self, path: &Path) -> std::io::Result<Vec<u8>> {
        self.files
            .get(path)
            .map(|(c, _)| c.clone())
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))
    }

    fn file_write(
        &mut self,
        path: &Path,
        contents: &[u8],
        create: bool,
    ) -> std::io::Result<SystemTime> {
        if !create && !self.files.contains_key(path) {
            return Err(std::io::Error::from(std::io::ErrorKind::NotFound));
        }
        let mtime = self.system_time + Duration::from_nanos(1);
        self.files
            .insert(path.to_path_buf(), (contents.to_vec(), mtime));
        Ok(mtime)
    }
}

// Action picked by the fuzzer for each step.
const A_KEY_CHAR: u32 = 40;
const A_KEY_NAMED: u32 = 20;
const A_MODIFIERS: u32 = 10;
const A_CLOSE: u32 = 2;
const A_TICK: u32 = 20;
const A_DRAW: u32 = 40;
const A_SCROLL: u32 = 20;
const A_MOUSE_MOVE: u32 = 20;
const A_MOUSE_BUTTON: u32 = 20;
const A_FILE_MODIFY: u32 = 10;
const A_FILE_DELETE: u32 = 5;
const A_FOCUS: u32 = 10;

fn random_mouse_pos(frng: &mut Frng, screen_size: [f32; 2]) -> Option<[f32; 2]> {
    let x_limit = (screen_size[0].max(0.0) as u32)
        .saturating_add(200)
        .max(4000);
    let y_limit = (screen_size[1].max(0.0) as u32)
        .saturating_add(200)
        .max(4000);
    let x = frng.u32_bounded(0, x_limit)? as f32 - 100.0;
    let y = frng.u32_bounded(0, y_limit)? as f32 - 100.0;
    Some([x, y])
}

// Each step: tick once (advancing time), then perform one randomly
// chosen action. Returns Some(()) if more entropy is available; None
// when the buffer is exhausted (Frng signals end-of-stream as None).
fn step(frng: &mut Frng, app: &mut App, io: &mut MockIO) -> Option<()> {
    if io.open_windows.is_empty() {
        return None;
    }
    let window_idx = frng.usize_bounded(0, io.open_windows.len() - 1)?;
    let window_id = io.open_windows[window_idx];

    let action = frng.weighted(&[
        A_KEY_CHAR,
        A_KEY_NAMED,
        A_MODIFIERS,
        A_CLOSE,
        A_TICK,
        A_DRAW,
        A_SCROLL,
        A_MOUSE_MOVE,
        A_MOUSE_BUTTON,
        A_FILE_MODIFY,
        A_FOCUS,
        A_FILE_DELETE,
    ])?;
    match action {
        0 => {
            // Most of the time, printable ASCII. Occasionally a
            // multi-byte UTF-8 char, to exercise mid-codepoint logic.
            let ch = if frng.u8_bounded(0, 15)? == 0 {
                // Pick from a small set of multi-byte chars.
                match frng.u8_bounded(0, 4)? {
                    0 => 'é', // 2 bytes
                    1 => 'ü',
                    2 => '€',  // 3 bytes
                    3 => '猫', // 3 bytes
                    _ => '🦀', // 4 bytes
                }
            } else {
                frng.u8_bounded(0x20, 0x7e)? as char
            };
            let mut buf = [0u8; 4];
            let s = ch.encode_utf8(&mut buf);
            let event = InputEvent::Key {
                state: ElementState::Pressed,
                logical_key: Key::Character(s),
            };
            app.input(io, window_id, event);
        }
        1 => {
            // Named key (Enter, Space, Backspace, Delete, plus a few
            // others to exercise unhandled-key paths).
            let named = match frng.u8_bounded(0, 7)? {
                0 => NamedKey::Enter,
                1 => NamedKey::Space,
                2 => NamedKey::Backspace,
                3 => NamedKey::Delete,
                4 => NamedKey::ArrowLeft,
                5 => NamedKey::ArrowRight,
                6 => NamedKey::ArrowUp,
                _ => NamedKey::ArrowDown,
            };
            let state = if frng.boolean()? {
                ElementState::Pressed
            } else {
                ElementState::Released
            };
            app.input(
                io,
                window_id,
                InputEvent::Key {
                    state,
                    logical_key: Key::Named(named),
                },
            );
        }
        2 => {
            // Random modifier state: any subset of Ctrl/Alt/Shift/Super.
            let bits = frng.u8_bounded(0, 0x0F)?;
            let m = ModifiersState {
                control: bits & 0b0001 != 0,
                alt: bits & 0b0010 != 0,
                shift: bits & 0b0100 != 0,
                super_: bits & 0b1000 != 0,
            };
            app.input(io, window_id, InputEvent::ModifiersChanged(m));
        }
        3 => {
            app.input(io, window_id, InputEvent::CloseRequested);
        }
        4 => {
            // Advance time by a fuzzer-chosen delta in [0, ~1s].
            let delta_us = frng.u32_bounded(0, 1_000_000)?;
            app.frame_start += Duration::from_micros(delta_us as u64);
            app.tick(io);
        }
        5 => {
            // Draw at a fuzzer-chosen screen size.
            if frng.boolean()? {
                io.screen_size = [
                    frng.u32_bounded(0, 4000)? as f32,
                    frng.u32_bounded(0, 4000)? as f32,
                ];
            }
            let mut drawing = Drawing::new(io.screen_size);
            app.draw(window_id, &mut drawing);
        }
        6 => {
            // Mouse wheel scroll. Map the raw byte into a signed scroll
            // amount roughly the size of a wheel notch, with occasional
            // larger jumps (smooth-scroll / pixel-delta sized).
            let raw = frng.u8_bounded(0, 200)? as f32;
            let y_offset = (raw - 100.0) / 10.0;
            app.input(io, window_id, InputEvent::MouseWheel { y_offset });
        }
        7 => {
            // Cursor movement is not an InputEvent in the real app; winit
            // updates the last cursor position, then App samples it on tick.
            app.mouse_position = random_mouse_pos(frng, io.screen_size)?;
        }
        8 => {
            // TODO exponenti delta from mouse_position instead
            let position = if frng.boolean()? {
                random_mouse_pos(frng, io.screen_size)?
            } else {
                app.mouse_position
            };
            let state = if frng.boolean()? {
                ElementState::Pressed
            } else {
                ElementState::Released
            };
            app.input(io, window_id, InputEvent::MouseButton { state, position });
        }
        9 => {
            let paths: Vec<PathBuf> = io.files.keys().cloned().collect();
            if !paths.is_empty() {
                let path = paths[frng.usize_bounded(0, paths.len() - 1)?].clone();
                let len = frng.usize_bounded(0, 64)?;
                let mut contents = Vec::with_capacity(len);
                for _ in 0..len {
                    contents.push(frng.u8_bounded(0x20, 0x7e)?);
                }
                let mtime = io
                    .files
                    .get(&path)
                    .map(|(_, mtime)| *mtime + Duration::from_nanos(1))
                    .unwrap_or(SystemTime::UNIX_EPOCH);
                io.files.insert(path, (contents, mtime));
            }
        }
        10 => {
            app.input(
                io,
                window_id,
                InputEvent::FocusChanged {
                    focused: frng.boolean()?,
                },
            );
        }
        11 => {
            // Delete a file out from under the app, as if it had been
            // removed on disk by another process. Subsequent file_mtime /
            // file_read calls for this path then return NotFound.
            let paths: Vec<PathBuf> = io.files.keys().cloned().collect();
            if !paths.is_empty() {
                let path = paths[frng.usize_bounded(0, paths.len() - 1)?].clone();
                io.files.remove(&path);
            }
        }
        _ => unreachable!(),
    }
    Some(())
}

pub fn fuzz_one(bytes: &[u8]) {
    let mut frng = Frng::new(bytes);
    let mut io = MockIO::new();
    let initial = io.fresh_window_id();
    io.open_windows.push(initial);
    let initial_path = PathBuf::from("fuzz.txt");
    io.files
        .insert(initial_path.clone(), (Vec::new(), SystemTime::UNIX_EPOCH));
    let mut app = App::new(initial, &mut io, Some(initial_path));

    loop {
        if step(&mut frng, &mut app, &mut io).is_none() {
            break;
        }
        if io.exited {
            break;
        }
    }

    app.assert_invariants();
}
