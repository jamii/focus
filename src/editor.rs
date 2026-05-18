use winit::{event::ElementState, keyboard::Key};

use crate::{
    app::{App, DocumentId, EditorId, IO, InputEvent},
    document::{Edit, Insert},
    text::Drawing,
};

pub struct Editor {
    pub document_id: DocumentId,
}

pub fn input(editor_id: EditorId, app: &App, _io: &mut dyn IO, event: InputEvent) {
    let mut edits = vec![];
    let editor = app.get_editor_mut(editor_id);
    let document_id = editor.document_id;
    let mut document = app.get_document_mut(document_id);
    match event {
        InputEvent::KeyboardInput {
            event: key_event, ..
        } if key_event.state == ElementState::Pressed => match key_event.logical_key.as_ref() {
            Key::Character(char) => {
                let end = document.text.len();
                edits.push(document.insert(vec![Insert {
                    pos: end,
                    text: char.into(),
                }]));
            }
            _ => {}
        },
        _ => {}
    }
    drop(editor);
    for editor in app.editors.values() {
        let mut editor = editor.borrow_mut();
        if editor.document_id == document_id {
            editor.handle_edits(&edits);
        }
    }
}

impl Editor {
    pub fn new(document_id: DocumentId) -> Self {
        Editor {
            document_id: document_id,
        }
    }

    pub fn draw(&self, app: &App, drawing: &mut Drawing) {
        app.get_document(self.document_id).draw(app, drawing);
    }

    pub fn handle_edits(&mut self, _edits: &[Edit]) {}
}
