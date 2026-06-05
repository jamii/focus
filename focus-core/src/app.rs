use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use bstr::BString;
use fontdue::{Font, FontSettings};

use crate::atlas::Atlas;
use crate::document::Document;
use crate::drawing::Drawing;
use crate::editor::Editor;
use crate::input::{ElementState, Key, ModifiersState};
use crate::window::Window;

pub struct App {
    font: Font,
    px_size: f32,
    pub atlas: Atlas,

    pub windows: HashMap<WindowId, Window>,

    next_editor_id: EditorId,
    pub editors: HashMap<EditorId, Editor>,

    next_document_id: DocumentId,
    pub documents: HashMap<DocumentId, Document>,

    pub(crate) modifiers: ModifiersState,

    // These can be set by Chrome.
    pub frame_start: Duration,
    pub mouse_position: [f32; 2],
}

#[derive(PartialEq, Eq, Hash, Clone, Copy, Debug)]
pub struct WindowId(pub usize);

#[derive(Clone, Copy, Debug)]
pub struct WindowSize {
    pub width: u32,
    pub height: u32,
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy, Debug)]
pub struct DocumentId(usize);

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy, Debug)]
pub struct EditorId(usize);

#[derive(Clone, Debug)]
pub enum InputEvent<'a> {
    CloseRequested,
    ModifiersChanged(ModifiersState),
    Key {
        state: ElementState,
        logical_key: Key<'a>,
    },
    MouseWheel {
        y_offset: f32,
    },
    MouseButton {
        state: ElementState,
        position: [f32; 2],
    },
    FocusChanged {
        focused: bool,
    },
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
    fn reload_atlas(&mut self, pixels: &[u8], size: [u32; 2]);
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

const FONT: &[u8] = include_bytes!("../deps/FiraCode-Regular.ttf");
const INITIAL_PX: f32 = 32.0;
const MIN_PX: f32 = 4.0;

impl App {
    pub fn assert_invariants(&self) {
        // TODO What should we assert for font/atlas?

        for document_id in self.documents.keys() {
            document_id.assert_invariants(self);
        }
        for editor_id in self.editors.keys() {
            editor_id.assert_invariants(self);
        }
        for window_id in self.windows.keys() {
            window_id.assert_invariants(self);
        }
    }

    pub fn new(initial_window_id: WindowId, io: &mut dyn IO, initial_path: Option<PathBuf>) -> App {
        let font = Font::from_bytes(FONT, FontSettings::default()).unwrap();
        let atlas = Atlas::build(&font, INITIAL_PX);
        io.reload_atlas(&atlas.pixels, atlas.size);
        let mut app = App {
            font,
            px_size: INITIAL_PX,
            atlas,
            windows: HashMap::new(),
            next_editor_id: EditorId(0),
            editors: HashMap::new(),
            next_document_id: DocumentId(0),
            documents: HashMap::new(),
            frame_start: Duration::ZERO,
            modifiers: ModifiersState::default(),
            mouse_position: [0.0, 0.0],
        };
        let document_id = match initial_path {
            Some(path) => app.insert_document(Document::from_file(path)),
            None => app.insert_document(Document::scratch()),
        };
        let editor_id = app.insert_editor(Editor::new(&app, document_id));
        app.windows
            .insert(initial_window_id, Window::new(editor_id));
        app
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
            } if *state == ElementState::Pressed && self.modifiers.control => {
                match *logical_key {
                    Key::Character("+") => {
                        self.px_size += 1.0;
                        self.rebuild_atlas(io);
                    }
                    Key::Character("-") => {
                        self.px_size = (self.px_size - 1.0).max(MIN_PX);
                        self.rebuild_atlas(io);
                    }
                    Key::Character("n") => {
                        self.insert_window_empty(io);
                    }
                    Key::Character("m") => {
                        let document_id = window_id.get(self).editor_id.get(self).document_id;
                        let editor_id_new = self.insert_editor(Editor::new(self, document_id));
                        self.insert_window(io, Window::new(editor_id_new));
                    }
                    _ => {
                        window_id.input(self, io, event);
                    }
                }
            }
            _ => {
                window_id.input(self, io, event);
            }
        }
    }

    pub fn tick(&mut self, io: &mut dyn IO) {
        let window_ids: Vec<_> = self.windows.keys().copied().collect();
        for window_id in window_ids {
            window_id.tick(self, io);
            io.request_redraw(window_id);
        }
    }

    pub fn draw(&mut self, window_id: WindowId, drawing: &mut Drawing) {
        window_id.draw(self, drawing);
    }

    fn rebuild_atlas(&mut self, io: &mut dyn IO) {
        self.atlas = Atlas::build(&self.font, self.px_size);
        io.reload_atlas(&self.atlas.pixels, self.atlas.size);
    }

    fn insert_window_empty(&mut self, io: &mut dyn IO) -> WindowId {
        let editor_id = self.insert_editor_empty();
        self.insert_window(io, Window::new(editor_id))
    }

    fn insert_window(&mut self, io: &mut dyn IO, window: Window) -> WindowId {
        let window_id = io.open_window(INITIAL_TITLE.to_string(), INITIAL_SIZE);
        self.windows.insert(window_id, window);
        window_id
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

impl WindowId {
    pub(crate) fn get<'a>(self, app: &'a App) -> &'a Window {
        app.windows.get(&self).unwrap()
    }
}

impl EditorId {
    pub(crate) fn get<'a>(self, app: &'a App) -> &'a Editor {
        app.editors.get(&self).unwrap()
    }

    pub(crate) fn get_mut<'a>(self, app: &'a mut App) -> &'a mut Editor {
        app.editors.get_mut(&self).unwrap()
    }
}

impl DocumentId {
    pub fn get<'a>(self, app: &'a App) -> &'a Document {
        app.documents.get(&self).unwrap()
    }

    pub(crate) fn get_mut<'a>(self, app: &'a mut App) -> &'a mut Document {
        app.documents.get_mut(&self).unwrap()
    }
}
