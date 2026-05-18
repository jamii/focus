use std::cell::{Ref, RefCell, RefMut};
use std::collections::HashMap;
use std::time::Duration;

use fontdue::{Font, FontSettings};
use winit::dpi::LogicalSize;
use winit::event::{ElementState, Modifiers};
use winit::keyboard::Key;
use winit::window::WindowId;

use crate::document::Document;
use crate::editor::Editor;
use crate::text::{Atlas, Drawing};
use crate::window::Window;

pub struct App {
    font: Font,
    px_size: f32,
    pub atlas: Atlas,

    pub windows: HashMap<WindowId, RefCell<Window>>,
    pub modifiers: Modifiers,

    next_editor_id: EditorId,
    pub editors: HashMap<EditorId, RefCell<Editor>>,

    next_document_id: DocumentId,
    pub documents: HashMap<DocumentId, RefCell<Document>>,
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy, Debug)]
pub struct DocumentId(usize);

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy, Debug)]
pub struct EditorId(usize);

pub type InputEvent = winit::event::WindowEvent;

// External effects. Mocked for testing/fuzzing.
pub trait IO {
    fn elapsed(&self) -> Duration;
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
    pub fn new(initial_window_id: WindowId, io: &mut dyn IO) -> App {
        let font = Font::from_bytes(FONT, FontSettings::default()).unwrap();
        let atlas = Atlas::build(&font, INITIAL_PX);
        io.reload_atlas(&atlas);
        let mut app = App {
            font,
            px_size: INITIAL_PX,
            atlas,
            windows: HashMap::new(),
            modifiers: Modifiers::default(),
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

    pub fn input(&mut self, window_id: WindowId, event: InputEvent, io: &mut dyn IO) {
        match &event {
            InputEvent::CloseRequested => {
                self.windows.remove(&window_id);
                io.close_window(window_id);
                if self.windows.is_empty() {
                    io.exit();
                }
            }
            InputEvent::ModifiersChanged(modifiers) => {
                println!("{modifiers:?}");
                self.modifiers = *modifiers;
            }
            InputEvent::KeyboardInput {
                event: key_event, ..
            } if key_event.state == ElementState::Pressed
                && self.modifiers.state().control_key() =>
            {
                match key_event.logical_key.as_ref() {
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
                        let editor_id = self.get_window(window_id).editor_id;
                        let document_id = self.get_editor(editor_id).document_id;
                        let editor_id_new = self.insert_editor(Editor::new(document_id));
                        self.insert_window(io, Window::new(editor_id_new));
                    }
                    _ => {
                        self.get_window_mut(window_id).input(self, io, event);
                    }
                }
            }
            _ => {
                self.get_window_mut(window_id).input(self, io, event);
            }
        }

        let mut document_edits = HashMap::new();
        for (document_id, document) in &self.documents {
            let mut document = document.borrow_mut();
            if let Some(edits) = document.queued_edits.take() {
                document.apply_edits(&edits);
                document_edits.insert(*document_id, edits);
            }
        }

        let mut editor_edits = HashMap::new();
        for (editor_id, editor) in &self.editors {
            let mut editor = editor.borrow_mut();
            if let Some(edits) = document_edits.get(&editor.document_id) {
                editor.handle_edits(edits);
                editor_edits.insert(*editor_id, edits);
            }
        }

        for (window_id, window) in &self.windows {
            let window = window.borrow();
            if editor_edits.contains_key(&window.editor_id) {
                io.request_redraw(*window_id);
            }
        }

        io.request_redraw(window_id);
    }

    pub fn tick(&mut self, _dt: f64, _io: &mut dyn IO) {
        // Future: cursor blink, scroll inertia, anything else that
        // depends on elapsed time goes here.
    }

    pub fn draw(&self, window_id: WindowId, drawing: &mut Drawing) {
        self.get_window(window_id).draw(self, drawing);
    }

    fn rebuild_atlas(&mut self, io: &mut dyn IO) {
        self.atlas = Atlas::build(&self.font, self.px_size);
        io.reload_atlas(&self.atlas);
        for (window_id, _) in self.windows.iter() {
            io.request_redraw(*window_id);
        }
    }

    pub fn get_window<'a>(&'a self, window_id: WindowId) -> Ref<'a, Window> {
        self.windows.get(&window_id).unwrap().borrow()
    }

    pub fn get_editor<'a>(&'a self, editor_id: EditorId) -> Ref<'a, Editor> {
        self.editors.get(&editor_id).unwrap().borrow()
    }

    pub fn get_document<'a>(&'a self, document_id: DocumentId) -> Ref<'a, Document> {
        self.documents.get(&document_id).unwrap().borrow()
    }

    pub fn get_window_mut<'a>(&'a self, window_id: WindowId) -> RefMut<'a, Window> {
        self.windows.get(&window_id).unwrap().borrow_mut()
    }

    pub fn get_editor_mut<'a>(&'a self, editor_id: EditorId) -> RefMut<'a, Editor> {
        self.editors.get(&editor_id).unwrap().borrow_mut()
    }

    pub fn get_document_mut<'a>(&'a self, document_id: DocumentId) -> RefMut<'a, Document> {
        self.documents.get(&document_id).unwrap().borrow_mut()
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
        self.insert_editor(Editor::new(document_id))
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
