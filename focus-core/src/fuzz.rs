// Fuzz harness for the editor.
//
// `fuzz_one` drives an empty `App` with a sequence of synthesized
// `InputEvent`s and ticks, both pulled from a `Frng` (see fuzz_gen.rs).
// Time advances by a fuzzer-chosen delta between events. The IO is mocked
// so the harness runs anywhere.
//
// This is the body of the honggfuzz target (see hfuzz/src/main.rs).

use std::collections::{BTreeMap, HashMap};
use std::mem::take;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime};

use bstr::{BStr, BString, ByteSlice};

use crate::app::{
    App, DirEntry, IO, ProcessId, ProcessPoll, RepoFiles, RepoMatch, RepoSearch, WindowSize,
};
use crate::buffer;
use crate::drawing::Drawing;
use crate::fuzz_gen::Frng;
use crate::input::{ButtonState, InputEvent, Key, ModifiersState, NamedKey, ScrollPhase};
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
    pub git_roots: Vec<PathBuf>,
    pub system_time: SystemTime,
    pub next_process_output: Vec<u8>,
    pub next_process_exit_code: Option<i32>,
    pub processes: Vec<MockProcess>,
    pub detached: Vec<DetachedProcess>,
}

// A scripted process: tests push bytes into `pending_output` and set
// `exit_code`; `process_poll` drains the former and reports the latter.
pub struct MockProcess {
    pub dir: PathBuf,
    pub command: BString,
    pub args: Vec<BString>,
    pub pending_output: Vec<u8>,
    pub exit_code: Option<i32>,
    pub killed: bool,
}

fn to_bstrings(args: &[&BStr]) -> Vec<BString> {
    args.iter().map(|arg| BString::from(arg.to_vec())).collect()
}

// A one-shot process spawned and forgotten. Nothing runs it; it is
// recorded so tests can assert what was handed to the shell.
pub struct DetachedProcess {
    pub dir: PathBuf,
    pub command: BString,
    pub args: Vec<BString>,
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
            git_roots: Vec::new(),
            system_time: SystemTime::UNIX_EPOCH,
            next_process_output: Vec::new(),
            next_process_exit_code: None,
            processes: Vec::new(),
            detached: Vec::new(),
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

    // The mock filesystem is a synthetic tree rooted at "/", so that is
    // also where home lives: pages that fall back to the home dir start at
    // the root of the tree the test built.
    fn home_dir(&mut self) -> PathBuf {
        PathBuf::from("/")
    }

    // The mock filesystem has no symlinks, so resolving `.` and `..`
    // textually is exactly what canonicalizing means here.
    fn canonical_path(&mut self, path: &Path) -> PathBuf {
        let mut canonical = PathBuf::from("/");
        for component in path.components() {
            match component {
                Component::Normal(part) => canonical.push(part),
                Component::ParentDir => {
                    canonical.pop();
                }
                Component::CurDir | Component::RootDir | Component::Prefix(_) => {}
            }
        }
        canonical
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

    fn repo_files(&mut self, dir: &Path) -> std::io::Result<RepoFiles> {
        let root = self.repo_root(dir);
        let mut relative_paths = Vec::new();
        for file in self.files.keys() {
            if let Ok(relative_path) = file.strip_prefix(&root)
                && !relative_path.as_os_str().is_empty()
            {
                relative_paths.push(relative_path.to_path_buf());
            }
        }
        relative_paths.sort();
        Ok(RepoFiles {
            root,
            relative_paths,
        })
    }

    fn repo_search(&mut self, dir: &Path, pattern: &BStr) -> std::io::Result<RepoSearch> {
        let root = self.repo_root(dir);
        let mut matches = Vec::new();
        if pattern.is_empty() {
            return Ok(RepoSearch { root, matches });
        }
        let mut entries: Vec<(PathBuf, &Vec<u8>)> = self
            .files
            .iter()
            .filter_map(|(file, (contents, _))| {
                let relative_path = file.strip_prefix(&root).ok()?;
                (!relative_path.as_os_str().is_empty())
                    .then(|| (relative_path.to_path_buf(), contents))
            })
            .collect();
        entries.sort();
        for (relative_path, contents) in entries {
            let contents = contents.as_bstr();
            let mut search_start = 0;
            while let Some(relative_start) = contents[search_start..].find(pattern) {
                let start = search_start + relative_start;
                let end = start + pattern.len();
                let line = contents[..start]
                    .iter()
                    .filter(|&&byte| byte == b'\n')
                    .count();
                let line_start = contents[..start]
                    .rfind_byte(b'\n')
                    .map(|ix| ix + 1)
                    .unwrap_or(0);
                let line_end = contents[start..]
                    .find_byte(b'\n')
                    .map(|ix| start + ix)
                    .unwrap_or(contents.len());
                matches.push(RepoMatch {
                    relative_path: relative_path.clone(),
                    line,
                    range: start..end,
                    line_text: contents[line_start..line_end].into(),
                });
                search_start = end;
            }
        }
        Ok(RepoSearch { root, matches })
    }

    fn repo_root(&mut self, dir: &Path) -> PathBuf {
        let mut current = Some(dir);
        while let Some(path) = current {
            if self.git_roots.iter().any(|root| root == path) {
                return path.to_path_buf();
            }
            current = path.parent();
        }
        dir.to_path_buf()
    }

    fn process_spawn(&mut self, dir: &Path, command: &BStr, args: &[&BStr]) -> ProcessId {
        self.processes.push(MockProcess {
            dir: dir.to_path_buf(),
            command: command.into(),
            args: to_bstrings(args),
            pending_output: take(&mut self.next_process_output),
            exit_code: self.next_process_exit_code.take(),
            killed: false,
        });
        ProcessId(self.processes.len() - 1)
    }

    fn process_poll(&mut self, id: ProcessId) -> ProcessPoll {
        let process = &mut self.processes[id.0];
        ProcessPoll {
            new_output: take(&mut process.pending_output),
            exit_code: process.exit_code,
        }
    }

    fn process_kill(&mut self, id: ProcessId) {
        let process = &mut self.processes[id.0];
        process.killed = true;
        // Match the real impl: a killed process reports an exit on later polls.
        if process.exit_code.is_none() {
            process.exit_code = Some(-1);
        }
    }

    fn process_spawn_detached(&mut self, dir: &Path, command: &BStr, args: &[&BStr]) {
        self.detached.push(DetachedProcess {
            dir: dir.to_path_buf(),
            command: command.into(),
            args: to_bstrings(args),
        });
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
const A_OPEN_REPO_FILE_PAGE: u32 = 10;
const A_OPEN_BUFFER_PAGE: u32 = 10;
const A_SEARCH_BUFFER_PAGE: u32 = 10;
const A_FILE_CREATE: u32 = 5;
const A_SEARCH_REPO_PAGE: u32 = 10;
const A_CHOOSE_COMMAND_PAGE: u32 = 10;
const A_PROCESS_OUTPUT: u32 = 10;
const A_DUPLICATE_PAGE: u32 = 10;
const A_OPEN_WINDOW: u32 = 5;

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

// Actions that need no window. A daemon with every window closed can
// still tick, and still sees files change underneath it, so the
// windowless step below runs these too.

fn act_tick(frng: &mut Frng, app: &mut App, io: &mut MockIO) -> Option<()> {
    // Advance time by a fuzzer-chosen delta in [0, ~1s].
    let delta_us = frng.u32_bounded(0, 1_000_000)?;
    io.frame_start += Duration::from_micros(delta_us as u64);
    let frame_start = io.frame_start;
    app.tick(io, frame_start);
    Some(())
}

fn act_file_modify(frng: &mut Frng, io: &mut MockIO) -> Option<()> {
    // Sorted so the pick depends only on the fuzz bytes, not on
    // HashMap iteration order, which changes from run to run and
    // would make crashes irreproducible.
    let mut paths: Vec<PathBuf> = io.files.keys().cloned().collect();
    paths.sort();
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
    Some(())
}

fn act_file_delete(frng: &mut Frng, io: &mut MockIO) -> Option<()> {
    // Delete a file out from under the app, as if it had been
    // removed on disk by another process. Subsequent file_mtime /
    // file_read calls for this path then return NotFound.
    let mut paths: Vec<PathBuf> = io.files.keys().cloned().collect();
    paths.sort();
    if !paths.is_empty() {
        let path_index = frng.usize_bounded(0, paths.len() - 1)?;
        io.files.remove(&paths[path_index]);
    }
    Some(())
}

fn act_file_create(frng: &mut Frng, io: &mut MockIO) -> Option<()> {
    // Create a file at a random path, so dirs appear and change
    // under the file open and repo file search pages.
    let mut path = PathBuf::from("/");
    let component_count = frng.usize_bounded(1, 3)?;
    for _ in 0..component_count {
        let ix = frng.usize_bounded(0, FUZZ_PATH_COMPONENTS.len() - 1)?;
        path.push(FUZZ_PATH_COMPONENTS[ix]);
    }
    let mtime = io.system_time + Duration::from_nanos(1);
    io.files.insert(path, (b"created".to_vec(), mtime));
    Some(())
}

// Open a window, as a daemon does when a client sends it a request.
fn act_open_window(frng: &mut Frng, app: &mut App, io: &mut MockIO) -> Option<()> {
    match frng.u8_bounded(0, 2)? {
        0 => {
            let mut paths: Vec<PathBuf> = io.files.keys().cloned().collect();
            paths.sort();
            if paths.is_empty() {
                window::open_scratch(app, io);
            } else {
                let path_index = frng.usize_bounded(0, paths.len() - 1)?;
                let buffer_id = buffer::from_file(app, io, paths.swap_remove(path_index));
                window::open_edit(app, io, buffer_id);
            }
        }
        1 => {
            // Script the completion process the launcher spawns, so that
            // its command list is non-empty once a tick has drained it.
            io.next_process_output = b"a\tcommand\nb\tcommand\n".to_vec();
            io.next_process_exit_code = Some(0);
            window::open_launcher(app, io);
        }
        _ => {
            window::open_scratch(app, io);
        }
    }
    Some(())
}

// Each step: perform one randomly chosen action. Returns Some(()) if
// more entropy is available; None when the buffer is exhausted (Frng
// signals end-of-stream as None).
fn step(frng: &mut Frng, app: &mut App, io: &mut MockIO) -> Option<()> {
    // Closing the last window leaves a live daemon waiting for a request,
    // not a dead app, so the run continues with only the actions that need
    // no window - one of which opens one again.
    if io.open_windows.is_empty() {
        let action = frng.weighted(&[
            A_TICK,
            A_FILE_MODIFY,
            A_FILE_DELETE,
            A_FILE_CREATE,
            A_OPEN_WINDOW,
        ])?;
        return match action {
            0 => act_tick(frng, app, io),
            1 => act_file_modify(frng, io),
            2 => act_file_delete(frng, io),
            3 => act_file_create(frng, io),
            _ => act_open_window(frng, app, io),
        };
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
        A_OPEN_REPO_FILE_PAGE,
        A_OPEN_BUFFER_PAGE,
        A_SEARCH_BUFFER_PAGE,
        A_FILE_CREATE,
        A_SEARCH_REPO_PAGE,
        A_CHOOSE_COMMAND_PAGE,
        A_PROCESS_OUTPUT,
        A_DUPLICATE_PAGE,
        A_OPEN_WINDOW,
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
            act_tick(frng, app, io)?;
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
            // Scrolling. Either a mouse wheel notch, or one step of a
            // touchpad gesture - whose phases the fuzzer picks freely, so
            // that it also produces the gestures a touchpad never sends
            // (moves with no start, ends with no gesture, ...).
            let raw = frng.u8_bounded(0, 200)? as f32;
            let amount = (raw - 100.0) / 10.0;
            let event = match frng.u8_bounded(0, 3)? {
                0 => InputEvent::MouseWheel { y_lines: amount },
                phase => InputEvent::TouchpadScroll {
                    y_pixels: amount * 10.0,
                    phase: match phase {
                        1 => ScrollPhase::Started,
                        2 => ScrollPhase::Moved,
                        _ => ScrollPhase::Ended,
                    },
                },
            };
            app.input(io, window_id, event);
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
            act_file_modify(frng, io)?;
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
            act_file_delete(frng, io)?;
        }
        12 => {
            // Push the FileOpen page (ctrl+o). Enter and ctrl+enter are then
            // reachable via A_MODIFIERS + A_KEY_NAMED.
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
            // Push the repo file search page (ctrl+p).
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
                    logical_key: Key::Character("p"),
                },
            );
            app.input(
                io,
                window_id,
                InputEvent::ModifiersChanged(ModifiersState::default()),
            );
        }
        14 => {
            // Push the open buffer page (alt+p).
            app.input(
                io,
                window_id,
                InputEvent::ModifiersChanged(ModifiersState {
                    alt: true,
                    ..ModifiersState::default()
                }),
            );
            app.input(
                io,
                window_id,
                InputEvent::Key {
                    state: ButtonState::Pressed,
                    logical_key: Key::Character("p"),
                },
            );
            app.input(
                io,
                window_id,
                InputEvent::ModifiersChanged(ModifiersState::default()),
            );
        }
        15 => {
            // Push the search buffer page (ctrl+f).
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
                    logical_key: Key::Character("f"),
                },
            );
            app.input(
                io,
                window_id,
                InputEvent::ModifiersChanged(ModifiersState::default()),
            );
        }
        16 => {
            act_file_create(frng, io)?;
        }
        17 => {
            // Switch the window to the repo search page (alt+f).
            app.input(
                io,
                window_id,
                InputEvent::ModifiersChanged(ModifiersState {
                    alt: true,
                    ..ModifiersState::default()
                }),
            );
            app.input(
                io,
                window_id,
                InputEvent::Key {
                    state: ButtonState::Pressed,
                    logical_key: Key::Character("f"),
                },
            );
            app.input(
                io,
                window_id,
                InputEvent::ModifiersChanged(ModifiersState::default()),
            );
        }
        18 => {
            // Push the dir picker (ctrl+m), which leads to the command
            // picker and then the runner page via enter chords.
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
                    logical_key: Key::Character("m"),
                },
            );
            app.input(
                io,
                window_id,
                InputEvent::ModifiersChanged(ModifiersState::default()),
            );
        }
        19 => {
            // Feed output / an exit code to a random spawned process, so
            // the runner page's parsing and exited paths are reachable.
            if !io.processes.is_empty() {
                let ix = frng.usize_bounded(0, io.processes.len() - 1)?;
                if frng.boolean()? {
                    let len = frng.usize_bounded(0, 32)?;
                    let mut output = Vec::with_capacity(len);
                    for _ in 0..len {
                        // Mostly printable ascii, with some newlines and
                        // colons to form path:line:col tokens.
                        output.push(match frng.u8_bounded(0, 6)? {
                            0 => b'\n',
                            1 => b':',
                            2 => frng.u8_bounded(b'0', b'9')?,
                            _ => frng.u8_bounded(0x20, 0x7e)?,
                        });
                    }
                    io.processes[ix].pending_output.extend_from_slice(&output);
                } else {
                    io.processes[ix].exit_code = Some(frng.u8_bounded(0, 2)? as i32);
                }
            }
        }
        20 => {
            // Open a copy of the current page in a new window (ctrl+n), so
            // every page kind gets duplicated under random input.
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
                    logical_key: Key::Character("n"),
                },
            );
            app.input(
                io,
                window_id,
                InputEvent::ModifiersChanged(ModifiersState::default()),
            );
        }
        21 => {
            act_open_window(frng, app, io)?;
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
    let buffer_id = buffer::from_file(&mut app, &mut io, initial_path);
    window::open_edit(&mut app, &mut io, buffer_id);

    while step(&mut frng, &mut app, &mut io).is_some() {
        if io.exited {
            break;
        }
    }

    app.assert_invariants();
}
