use std::ffi::OsString;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use bstr::{BStr, BString};

use crate::buffer::{self, Buffers};
use crate::drawing::Drawing;
use crate::editor::{self, Editors};
use crate::input::{ButtonState, InputEvent, Key, ModifiersState};
use crate::page::{self, Pages};
use crate::window::{self, WindowId, Windows};

pub struct App {
    font_size: f32,
    cell_size: [u32; 2],

    pub windows: Windows,
    pub pages: Pages,
    pub editors: Editors,
    pub buffers: Buffers,

    pub(crate) search_buffer_text: BString,
    pub(crate) modifiers: ModifiersState,
    pub(crate) frame_start: Duration,
    // Incremented on every successful file save, so the runner page can
    // restart its command when any file changes.
    pub(crate) save_count: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct WindowSize {
    pub width: u32,
    pub height: u32,
}

pub const INITIAL_TITLE: &str = "focus";
pub const INITIAL_SIZE: WindowSize = WindowSize {
    width: 800,
    height: 600,
};

// External effects. Mocked for testing/fuzzing.
pub trait IO {
    fn open_window(&mut self, window_id: WindowId, title: String, size: WindowSize);
    fn close_window(&mut self, window_id: WindowId);
    fn set_window_title(&mut self, window_id: WindowId, title: String);
    fn request_redraw(&mut self, window_id: WindowId);
    /// Rebuild the glyph atlas at the given font size and return the
    /// resulting character cell size in pixels `[width, height]`.
    fn rebuild_atlas(&mut self, font_size: f32) -> [u32; 2];
    fn get_clipboard_text(&mut self) -> Option<BString>;
    fn set_clipboard_text(&mut self, text: BString);
    fn exit(&mut self);

    fn file_mtime(&mut self, path: &Path) -> std::io::Result<SystemTime>;
    fn file_read(&mut self, path: &Path) -> std::io::Result<Vec<u8>>;
    /// Write `contents` to `path`, truncating any existing file. If `create` is
    /// false and the file does not exist, returns Err(NotFound). Returns the
    /// post-write mtime on success.
    fn file_write(
        &mut self,
        path: &Path,
        contents: &[u8],
        create: bool,
    ) -> std::io::Result<SystemTime>;
    /// Read at most `limit` bytes from the start of the file.
    fn file_read_prefix(&mut self, path: &Path, limit: usize) -> std::io::Result<Vec<u8>>;
    /// Create the file, and any missing parent dirs, if it does not already
    /// exist. Does not truncate an existing file.
    fn file_create(&mut self, path: &Path) -> std::io::Result<()>;

    fn home_dir(&mut self) -> PathBuf;
    /// Resolve `path` to the one name the filesystem knows the file by:
    /// no `.` or `..`, and no symlinks. Two spellings of one file must
    /// produce the same result, or they end up in two buffers that save
    /// over each other. Returns `path` unchanged if it cannot be
    /// resolved - a file whose parent dir does not exist, say.
    fn canonical_path(&mut self, path: &Path) -> PathBuf;
    fn dir_list(&mut self, path: &Path) -> std::io::Result<Vec<DirEntry>>;
    fn repo_files(&mut self, dir: &Path) -> std::io::Result<RepoFiles>;
    /// Search every file in the repo containing `dir` for the literal string
    /// `pattern`. Returns one match per occurrence, in file order.
    fn repo_search(&mut self, dir: &Path, pattern: &BStr) -> std::io::Result<RepoSearch>;
    /// The root of the repo containing `dir`, or `dir` itself if there is
    /// no containing repo.
    fn repo_root(&mut self, dir: &Path) -> PathBuf;

    /// Spawn `command` as a shell command in `dir`, with stdout and stderr
    /// merged into one stream. `args` are passed to the shell as `$argv`,
    /// so text can be handed over without any quoting.
    fn process_spawn(&mut self, dir: &Path, command: &BStr, args: &[&BStr]) -> ProcessId;
    /// Drain any output produced since the last poll. `exit_code` is Some
    /// once the process has exited.
    fn process_poll(&mut self, id: ProcessId) -> ProcessPoll;
    fn process_kill(&mut self, id: ProcessId);
    /// As `process_spawn`, but for a short one-shot command: the output is
    /// discarded and the process is reaped when it exits. There is no
    /// ProcessId, so nothing can poll or kill it.
    fn process_spawn_detached(&mut self, dir: &Path, command: &BStr, args: &[&BStr]);
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub struct ProcessId(pub usize);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessPoll {
    pub new_output: Vec<u8>,
    pub exit_code: Option<i32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirEntry {
    pub name: OsString,
    pub is_dir: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoFiles {
    pub root: PathBuf,
    pub relative_paths: Vec<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoSearch {
    pub root: PathBuf,
    pub matches: Vec<RepoMatch>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoMatch {
    pub relative_path: PathBuf,
    /// 0-based index of the line containing the start of the match.
    pub line: usize,
    /// Byte range of the match within the file.
    pub range: Range<usize>,
    /// Text of the line containing the start of the match, without the
    /// trailing newline.
    pub line_text: BString,
}

const FONT_SIZE_INIT: f32 = 32.0;
const FONT_SIZE_MIN: f32 = 4.0;

impl App {
    pub fn assert_invariants(&self) {
        buffer::assert_invariants(self);
        editor::assert_invariants(self);
        page::assert_invariants(self);
        window::assert_invariants(self);
    }

    pub fn new(io: &mut dyn IO) -> App {
        let cell_size = io.rebuild_atlas(FONT_SIZE_INIT);
        App {
            font_size: FONT_SIZE_INIT,
            cell_size,
            windows: Windows::new(),
            pages: Pages::new(),
            editors: Editors::new(),
            buffers: Buffers::new(),
            search_buffer_text: BString::default(),
            frame_start: Duration::ZERO,
            save_count: 0,
            modifiers: ModifiersState::default(),
        }
    }

    pub fn tick(&mut self, io: &mut dyn IO, frame_start: Duration) {
        self.frame_start = frame_start;
        let window_ids: Vec<_> = self
            .windows
            .open
            .iter()
            .filter_map(|(window_id, open)| open.then_some(window_id))
            .collect();
        for window_id in window_ids {
            window_id.tick(self, io);
            io.request_redraw(window_id);
        }
    }

    pub fn input(&mut self, io: &mut dyn IO, window_id: WindowId, event: InputEvent<'_>) {
        let handled = match &event {
            InputEvent::ModifiersChanged(modifiers) => {
                self.modifiers = *modifiers;
                true
            }
            InputEvent::Key {
                state, logical_key, ..
            } if *state == ButtonState::Pressed && self.modifiers.control => match *logical_key {
                Key::Character("+") => {
                    self.font_size += 1.0;
                    self.rebuild_atlas(io);
                    true
                }
                Key::Character("-") => {
                    self.font_size = (self.font_size - 1.0).max(FONT_SIZE_MIN);
                    self.rebuild_atlas(io);
                    true
                }
                _ => false,
            },
            _ => false,
        };

        if !handled {
            window_id.input(self, io, event);
        }
    }

    pub fn draw(&mut self, window_id: WindowId, drawing: &mut Drawing) {
        window_id.draw(self, drawing);
    }

    fn rebuild_atlas(&mut self, io: &mut dyn IO) {
        self.cell_size = io.rebuild_atlas(self.font_size);
    }

    pub fn cell_size(&self) -> [u32; 2] {
        self.cell_size
    }

    /// Top-left screen position of the cell at the given grid coords.
    pub(crate) fn screen_from_grid(&self, grid: [usize; 2]) -> [f32; 2] {
        [
            (grid[0] as f32) * (self.cell_size[0] as f32),
            (grid[1] as f32) * (self.cell_size[1] as f32),
        ]
    }

    /// Grid cell containing the given screen position. Floor-divides, so
    /// a screen position on a cell boundary lands in the cell to its
    /// right / below.
    pub(crate) fn grid_from_screen(&self, screen: [f32; 2]) -> [i32; 2] {
        [
            (screen[0] / self.cell_size[0] as f32).floor() as i32,
            (screen[1] / self.cell_size[1] as f32).floor() as i32,
        ]
    }
}
