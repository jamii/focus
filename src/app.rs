use std::cell::{Ref, RefCell, RefMut};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use fontdue::{Font, FontSettings};
use winit::dpi::LogicalSize;
use winit::event::ElementState;
use winit::keyboard::{Key, ModifiersState};

use crate::atlas::Atlas;
use crate::document::Document;
use crate::drawing::Drawing;
use crate::editor::Editor;
use crate::window::Window;

pub struct App {
    font: Font,
    px_size: f32,
    pub atlas: Atlas,

    pub windows: HashMap<WindowId, RefCell<Window>>,
    pub modifiers: ModifiersState,

    next_editor_id: EditorId,
    pub editors: HashMap<EditorId, RefCell<Editor>>,

    next_document_id: DocumentId,
    pub documents: HashMap<DocumentId, RefCell<Document>>,
}

#[derive(PartialEq, Eq, Hash, Clone, Copy, Debug)]
pub struct WindowId(pub winit::window::WindowId);

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy, Debug)]
pub struct DocumentId(usize);

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy, Debug)]
pub struct EditorId(usize);

#[derive(Clone, Debug)]
pub enum InputEvent {
    CloseRequested,
    ModifiersChanged(ModifiersState),
    Key {
        state: ElementState,
        logical_key: Key,
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

// External effects. Mocked for testing/fuzzing.
pub trait IO {
    fn frame_start(&self) -> Duration;
    fn open_window(&mut self, title: String, size: LogicalSize<u32>) -> WindowId;
    fn close_window(&mut self, window_id: WindowId);
    fn set_window_title(&mut self, window_id: WindowId, title: String);
    fn request_redraw(&mut self, window_id: WindowId);
    fn reload_atlas(&mut self, atlas: &Atlas);
    fn mouse_position(&self) -> [f32; 2];
    fn get_clipboard_text(&mut self) -> Option<String>;
    fn set_clipboard_text(&mut self, text: String);
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
pub const INITIAL_TITLE: &str = "focus";
pub const INITIAL_SIZE: LogicalSize<u32> = LogicalSize {
    width: 800,
    height: 600,
};

impl App {
    pub fn assert_invariants(&self) {
        // TODO What should we assert for font/atlas?

        for document in self.documents.values() {
            document.borrow().assert_invariants();
        }
        for editor in self.editors.values() {
            editor.borrow().assert_invariants(self);
        }
        for window in self.windows.values() {
            window.borrow().assert_invariants();
        }
    }

    pub fn new(initial_window_id: WindowId, io: &mut dyn IO, initial_path: Option<PathBuf>) -> App {
        let font = Font::from_bytes(FONT, FontSettings::default()).unwrap();
        let atlas = Atlas::build(&font, INITIAL_PX);
        io.reload_atlas(&atlas);
        let mut app = App {
            font,
            px_size: INITIAL_PX,
            atlas,
            windows: HashMap::new(),
            modifiers: ModifiersState::default(),
            next_editor_id: EditorId(0),
            editors: HashMap::new(),
            next_document_id: DocumentId(0),
            documents: HashMap::new(),
        };
        let document_id = match initial_path {
            Some(path) => app.insert_document(Document::from_file(path)),
            None => app.insert_document(Document::scratch()),
        };
        let editor_id = app.insert_editor(Editor::new(document_id, &app));
        app.windows
            .insert(initial_window_id, RefCell::new(Window::new(editor_id)));
        app
    }

    pub fn input(&mut self, io: &mut dyn IO, window_id: WindowId, event: InputEvent) {
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
            } if *state == ElementState::Pressed && self.modifiers.control_key() => {
                match logical_key.as_ref() {
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
                        let editor_id_new = self.insert_editor(Editor::new(document_id, self));
                        self.insert_window(io, Window::new(editor_id_new));
                    }
                    _ => {
                        window_id.get_mut(self).input(self, io, event);
                    }
                }
            }
            _ => {
                window_id.get_mut(self).input(self, io, event);
            }
        }

        self.flush_queued_edits();
    }

    pub fn tick(&mut self, io: &mut dyn IO) {
        for (window_id, window) in &self.windows {
            window.borrow_mut().tick(self, io);
            io.request_redraw(*window_id);
        }

        self.flush_queued_edits();
    }

    fn flush_queued_edits(&mut self) {
        let mut document_diffs = HashMap::new();
        for (document_id, document) in &self.documents {
            let mut document = document.borrow_mut();
            if let Some(edits) = document.queued_edits.take() {
                let diff = document.apply_edits(&edits);
                document_diffs.insert(*document_id, diff);
            }
        }

        let mut editor_diffs = HashMap::new();
        for (editor_id, editor) in &self.editors {
            let mut editor = editor.borrow_mut();
            if let Some(diff) = document_diffs.get(&editor.document_id) {
                editor.handle_edits(self, diff);
                editor_diffs.insert(*editor_id, diff);
            }
        }
    }

    pub fn draw(&mut self, window_id: WindowId, drawing: &mut Drawing) {
        window_id.get_mut(self).draw(self, drawing);
    }

    fn rebuild_atlas(&mut self, io: &mut dyn IO) {
        self.atlas = Atlas::build(&self.font, self.px_size);
        io.reload_atlas(&self.atlas);
    }

    fn insert_window_empty(&mut self, io: &mut dyn IO) -> WindowId {
        let editor_id = self.insert_editor_empty();
        self.insert_window(io, Window::new(editor_id))
    }

    fn insert_window(&mut self, io: &mut dyn IO, window: Window) -> WindowId {
        let window_id = io.open_window(INITIAL_TITLE.to_string(), INITIAL_SIZE);
        self.windows.insert(window_id, RefCell::new(window));
        window_id
    }

    pub fn insert_editor_empty(&mut self) -> EditorId {
        let document_id = self.insert_document_empty();
        self.insert_editor(Editor::new(document_id, self))
    }

    pub fn insert_editor(&mut self, editor: Editor) -> EditorId {
        let editor_id = self.next_editor_id;
        self.next_editor_id.0 += 1;
        self.editors.insert(editor_id, RefCell::new(editor));
        editor_id
    }

    pub fn insert_document_empty(&mut self) -> DocumentId {
        self.insert_document(Document::scratch())
    }

    pub fn insert_document(&mut self, document: Document) -> DocumentId {
        let document_id = self.next_document_id;
        self.next_document_id.0 += 1;
        self.documents.insert(document_id, RefCell::new(document));
        document_id
    }
}

impl WindowId {
    pub fn get<'a>(self, app: &'a App) -> Ref<'a, Window> {
        app.windows.get(&self).unwrap().borrow()
    }

    pub fn get_mut<'a>(self, app: &'a App) -> RefMut<'a, Window> {
        app.windows.get(&self).unwrap().borrow_mut()
    }
}

impl EditorId {
    pub fn get<'a>(self, app: &'a App) -> Ref<'a, Editor> {
        app.editors.get(&self).unwrap().borrow()
    }

    pub fn get_mut<'a>(self, app: &'a App) -> RefMut<'a, Editor> {
        app.editors.get(&self).unwrap().borrow_mut()
    }
}

impl DocumentId {
    pub fn get<'a>(self, app: &'a App) -> Ref<'a, Document> {
        app.documents.get(&self).unwrap().borrow()
    }

    pub fn get_mut<'a>(self, app: &'a App) -> RefMut<'a, Document> {
        app.documents.get(&self).unwrap().borrow_mut()
    }
}
