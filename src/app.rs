use std::cell::{Ref, RefCell, RefMut};
use std::collections::HashMap;
use std::time::Duration;

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
}

// External effects. Mocked for testing/fuzzing.
pub trait IO {
    fn frame_start(&self) -> Duration;
    fn open_window(&mut self, title: String, size: LogicalSize<u32>) -> WindowId;
    fn close_window(&mut self, window_id: WindowId);
    fn set_window_title(&mut self, window_id: WindowId, title: String);
    fn request_redraw(&mut self, window_id: WindowId);
    fn reload_atlas(&mut self, atlas: &Atlas);
    fn exit(&mut self);
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

    pub fn new(initial_window_id: WindowId, io: &mut dyn IO) -> App {
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
        let editor_id = app.insert_editor_empty();
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

        for (window_id, window) in &self.windows {
            let window = window.borrow();
            if editor_diffs.contains_key(&window.editor_id) {
                io.request_redraw(*window_id);
            }
        }

        io.request_redraw(window_id);
    }

    pub fn tick(&mut self, io: &mut dyn IO) {
        for (window_id, window) in &self.windows {
            let mut redraw = false;
            window.borrow_mut().tick(self, io, &mut redraw);
            if redraw {
                io.request_redraw(*window_id);
            }
        }
    }

    pub fn draw(&mut self, window_id: WindowId, drawing: &mut Drawing) {
        window_id.get_mut(self).draw(self, drawing);
    }

    fn rebuild_atlas(&mut self, io: &mut dyn IO) {
        self.atlas = Atlas::build(&self.font, self.px_size);
        io.reload_atlas(&self.atlas);
        for (window_id, _) in self.windows.iter() {
            io.request_redraw(*window_id);
        }
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
        self.insert_document(Document::new())
    }

    pub fn insert_document(&mut self, document: Document) -> DocumentId {
        let document_id = self.next_document_id;
        self.next_document_id.0 += 1;
        self.documents.insert(document_id, RefCell::new(document));
        document_id
    }
}
