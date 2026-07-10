// Fuzz harness for the editor.
//
// `fuzz_one` drives an empty `App` with a sequence of synthesized
// `InputEvent`s and ticks, both pulled from a `Frng` (see fuzz_gen.rs).
// Time advances by a fuzzer-chosen delta between events. The IO is mocked
// so the harness runs anywhere.
//
// This is the body of the honggfuzz target (see hfuzz/src/main.rs).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use bstr::BString;

use crate::app::{App, DirEntry, IO, WindowSize};
use crate::buffer;
use crate::drawing::Drawing;
use crate::fuzz_gen::Frng;
use crate::input::{ButtonState, InputEvent, Key, ModifiersState, NamedKey};
use crate::window::{self, WindowId};

// Mock IO: tracks open windows and advances `frame_start` by whatever
// the harness pushes via `advance`.
pub struct MockIO {
    pub open_windows: Vec<WindowId>,
    pub exited: bool,
    pub screen_size: [f32; 2],
    pub frame_start: Duration,
    pub mouse_position: [f32; 2],
    pub clipboard: Option<BString>,
    pub files: HashMap<PathBuf, (Vec<u8>, SystemTime)>,
    pub system_time: SystemTime,
}

impl MockIO {
    pub fn new() -> Self {
        MockIO {
            open_windows: Vec::new(),
            exited: false,
            screen_size: [0.0, 0.0],
            frame_start: Duration::ZERO,
            mouse_position: [0.0, 0.0],
            clipboard: None,
            files: HashMap::new(),
            system_time: SystemTime::UNIX_EPOCH,
        }
    }
}

impl IO for MockIO {
    fn open_window(&mut self, window_id: WindowId, _title: String, _size: WindowSize) {
        assert!(!self.open_windows.contains(&window_id));
        self.open_windows.push(window_id);
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

    fn rebuild_atlas(&mut self, font_size: f32) -> [u32; 2] {
        // TODO not sure exactly how to calculate cell sizes
        [(font_size / 2.0).floor() as u32, font_size as u32]
    }

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

    fn file_read_prefix(&mut self, path: &Path, limit: usize) -> std::io::Result<Vec<u8>> {
        self.file_read(path).map(|mut contents| {
            contents.truncate(limit);
            contents
        })
    }

    // Parent dirs exist implicitly, so only the file needs creating.
    fn file_create(&mut self, path: &Path) -> std::io::Result<()> {
        if !self.files.contains_key(path) {
            let mtime = self.system_time + Duration::from_nanos(1);
            self.files.insert(path.to_path_buf(), (Vec::new(), mtime));
        }
        Ok(())
    }

    fn current_dir(&mut self) -> PathBuf {
        PathBuf::from("/")
    }

    // Directories exist implicitly: `path` is a dir iff it is the root or a
    // proper ancestor of some file in `files`.
    fn dir_list(&mut self, path: &Path) -> std::io::Result<Vec<DirEntry>> {
        // An empty path would strip_prefix-match every file.
        if path.as_os_str().is_empty() {
            return Err(std::io::Error::from(std::io::ErrorKind::NotFound));
        }
        let mut entries = BTreeMap::new();
        for file in self.files.keys() {
            if let Ok(rest) = file.strip_prefix(path) {
                let mut components = rest.components();
                if let Some(first) = components.next() {
                    let is_dir = components.next().is_some();
                    *entries.entry(first.as_os_str().to_owned()).or_insert(false) |= is_dir;
                }
            }
        }
        if entries.is_empty() && path != Path::new("/") {
            return Err(std::io::Error::from(std::io::ErrorKind::NotFound));
        }
        Ok(entries
            .into_iter()
            .map(|(name, is_dir)| DirEntry { name, is_dir })
            .collect())
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
const A_OPEN_FILE_PAGE: u32 = 10;
const A_FILE_CREATE: u32 = 5;

// Small pool of path components for A_FILE_CREATE, so created files
// sometimes collide with the seeded tree and sometimes add new dirs.
const FUZZ_PATH_COMPONENTS: &[&str] = &["dir", "sub", "a", "b.txt", "c.rs", "Émile"];

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
        A_OPEN_FILE_PAGE,
        A_FILE_CREATE,
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
                state: ButtonState::Pressed,
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
                ButtonState::Pressed
            } else {
                ButtonState::Released
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
            io.frame_start += Duration::from_micros(delta_us as u64);
            let frame_start = io.frame_start;
            app.tick(io, frame_start);
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
            let position = random_mouse_pos(frng, io.screen_size)?;
            io.mouse_position = position;
            app.input(io, window_id, InputEvent::MouseMoved { position });
        }
        8 => {
            let state = if frng.boolean()? {
                ButtonState::Pressed
            } else {
                ButtonState::Released
            };
            let position = io.mouse_position;
            app.input(io, window_id, InputEvent::MouseButton { state, position });
        }
        9 => {
            let mut paths: Vec<PathBuf> = io.files.keys().cloned().collect();
            if !paths.is_empty() {
                let path_index = frng.usize_bounded(0, paths.len() - 1)?;
                let path = paths.swap_remove(path_index);
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
                let path_index = frng.usize_bounded(0, paths.len() - 1)?;
                io.files.remove(&paths[path_index]);
            }
        }
        12 => {
            // Switch the window to the FileOpen page (ctrl+o). Enter and
            // ctrl+enter are then reachable via A_MODIFIERS + A_KEY_NAMED.
            app.input(
                io,
                window_id,
                InputEvent::ModifiersChanged(ModifiersState {
                    control: true,
                    ..ModifiersState::default()
                }),
            );
            app.input(
                io,
                window_id,
                InputEvent::Key {
                    state: ButtonState::Pressed,
                    logical_key: Key::Character("o"),
                },
            );
            app.input(
                io,
                window_id,
                InputEvent::ModifiersChanged(ModifiersState::default()),
            );
        }
        13 => {
            // Create a file at a random path, so dirs appear and change
            // under the FileOpen page.
            let mut path = PathBuf::from("/");
            let component_count = frng.usize_bounded(1, 3)?;
            for _ in 0..component_count {
                let ix = frng.usize_bounded(0, FUZZ_PATH_COMPONENTS.len() - 1)?;
                path.push(FUZZ_PATH_COMPONENTS[ix]);
            }
            let mtime = io.system_time + Duration::from_nanos(1);
            io.files.insert(path, (b"created".to_vec(), mtime));
        }
        _ => unreachable!(),
    }
    Some(())
}

pub fn fuzz_one(bytes: &[u8]) {
    let mut frng = Frng::new(bytes);
    let mut io = MockIO::new();
    let initial_path = PathBuf::from("/fuzz.txt");
    // A small file tree for the FileOpen page to browse.
    for path in ["/fuzz.txt", "/other.rs", "/dir/a", "/dir/sub/b.txt"] {
        io.files
            .insert(PathBuf::from(path), (Vec::new(), SystemTime::UNIX_EPOCH));
    }
    let mut app = App::new(&mut io);
    let buffer_id = buffer::from_file(&mut app, initial_path);
    window::open_edit(&mut app, &mut io, buffer_id);

    while step(&mut frng, &mut app, &mut io).is_some() {
        if io.exited {
            break;
        }
    }

    app.assert_invariants();
}
