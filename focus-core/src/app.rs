use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use bstr::BString;

use crate::document::{Document, DocumentId};
use crate::drawing::Drawing;
use crate::editor::{Editor, EditorId};
use crate::input::{ButtonState, InputEvent, Key, ModifiersState};
use crate::page::{Page, PageId};
use crate::window::{Window, WindowId};

pub struct App {
    font_size: f32,
    cell_size: [u32; 2],

    pub windows: HashMap<WindowId, Window>,

    next_page_id: PageId,
    pub pages: HashMap<PageId, Page>,

    next_editor_id: EditorId,
    pub editors: HashMap<EditorId, Editor>,

    next_document_id: DocumentId,
    pub documents: HashMap<DocumentId, Document>,

    pub(crate) modifiers: ModifiersState,
    pub(crate) frame_start: Duration,
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
    fn open_window(&mut self, title: String, size: WindowSize) -> WindowId;
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
}

const FONT_SIZE_INIT: f32 = 32.0;
const FONT_SIZE_MIN: f32 = 4.0;

impl App {
    pub fn assert_invariants(&self) {
        for document_id in self.documents.keys() {
            document_id.get(self).assert_invariants();
        }
        for page_id in self.pages.keys() {
            page_id.get(self).assert_invariants();
        }
        for editor_id in self.editors.keys() {
            editor_id.get(self).assert_invariants(self);
        }
        for window_id in self.windows.keys() {
            window_id.get(self).assert_invariants();
        }
    }

    pub fn new(initial_window_id: WindowId, io: &mut dyn IO, initial_path: Option<PathBuf>) -> App {
        let cell_size = io.rebuild_atlas(FONT_SIZE_INIT);
        let mut app = App {
            font_size: FONT_SIZE_INIT,
            cell_size,
            windows: HashMap::new(),
            next_page_id: PageId(0),
            pages: HashMap::new(),
            next_editor_id: EditorId(0),
            editors: HashMap::new(),
            next_document_id: DocumentId(0),
            documents: HashMap::new(),
            frame_start: Duration::ZERO,
            modifiers: ModifiersState::default(),
        };
        let document_id = match initial_path {
            Some(path) => app.insert_document(Document::from_file(path)),
            None => app.insert_document(Document::scratch()),
        };
        let editor_id = app.insert_editor(Editor::new(&app, document_id));
        let page_id = app.insert_page_single(editor_id);
        app.windows.insert(initial_window_id, Window::new(page_id));
        app
    }

    pub fn tick(&mut self, io: &mut dyn IO, frame_start: Duration) {
        self.frame_start = frame_start;
        let window_ids: Vec<_> = self.windows.keys().copied().collect();
        for window_id in window_ids {
            window_id.tick(self, io);
            io.request_redraw(window_id);
        }
    }

    pub fn input(&mut self, io: &mut dyn IO, window_id: WindowId, event: InputEvent<'_>) {
        match &event {
            InputEvent::CloseRequested => {
                self.windows.remove(&window_id);
                io.close_window(window_id);
                if self.windows.is_empty() {
                    io.exit();
                }
            }
            InputEvent::ModifiersChanged(modifiers) => {
                self.modifiers = *modifiers;
            }
            InputEvent::Key {
                state, logical_key, ..
            } if *state == ButtonState::Pressed && self.modifiers.control => match *logical_key {
                Key::Character("+") => {
                    self.font_size += 1.0;
                    self.rebuild_atlas(io);
                }
                Key::Character("-") => {
                    self.font_size = (self.font_size - 1.0).max(FONT_SIZE_MIN);
                    self.rebuild_atlas(io);
                }
                Key::Character("n") => {
                    self.insert_window_empty(io);
                }
                _ => {
                    window_id.input(self, io, event);
                }
            },
            _ => {
                window_id.input(self, io, event);
            }
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

    fn insert_window_empty(&mut self, io: &mut dyn IO) -> WindowId {
        let page_id = self.insert_page_single_empty();
        self.insert_window(io, Window::new(page_id))
    }

    fn insert_window(&mut self, io: &mut dyn IO, window: Window) -> WindowId {
        let window_id = io.open_window(INITIAL_TITLE.to_string(), INITIAL_SIZE);
        self.windows.insert(window_id, window);
        window_id
    }

    pub(crate) fn insert_page_single_empty(&mut self) -> PageId {
        let editor_id = self.insert_editor_empty();
        self.insert_page_single(editor_id)
    }

    pub(crate) fn insert_page_single(&mut self, editor_id: EditorId) -> PageId {
        let page = Page::new_single(editor_id, self);
        self.insert_page(page)
    }

    pub(crate) fn insert_page(&mut self, page: Page) -> PageId {
        let page_id = self.next_page_id;
        self.next_page_id.0 += 1;
        self.pages.insert(page_id, page);
        page_id
    }

    pub(crate) fn insert_editor_empty(&mut self) -> EditorId {
        let document_id = self.insert_document_empty();
        self.insert_editor(Editor::new(self, document_id))
    }

    pub(crate) fn insert_editor(&mut self, editor: Editor) -> EditorId {
        let editor_id = self.next_editor_id;
        self.next_editor_id.0 += 1;
        self.editors.insert(editor_id, editor);
        editor_id
    }

    pub(crate) fn insert_document_empty(&mut self) -> DocumentId {
        self.insert_document(Document::scratch())
    }

    pub(crate) fn insert_document(&mut self, document: Document) -> DocumentId {
        let document_id = self.next_document_id;
        self.next_document_id.0 += 1;
        self.documents.insert(document_id, document);
        document_id
    }
}
