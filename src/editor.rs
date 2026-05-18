use winit::{event::ElementState, keyboard::Key};

use crate::{
    app::{App, DocumentId, IO, InputEvent},
    document::Insert,
    text::Drawing,
};

pub struct Editor {
    pub document_id: DocumentId,
}
impl Editor {
    pub fn new(document_id: DocumentId) -> Self {
        Editor {
            document_id: document_id,
        }
    }

    pub fn input(&mut self, app: &App, _io: &mut dyn IO, event: InputEvent) {
        let mut document = app.get_document_mut(self.document_id);
        match event {
            InputEvent::KeyboardInput {
                event: key_event, ..
            } if key_event.state == ElementState::Pressed => match key_event.logical_key.as_ref() {
                Key::Character(char) => {
                    let end = document.text.len();
                    document.insert(vec![Insert {
                        pos: end,
                        text: char.into(),
                    }]);
                }
                _ => {}
            },
            _ => {}
        }
    }

    pub fn draw(&self, app: &App, drawing: &mut Drawing) {
        app.get_document(self.document_id).draw(app, drawing);
    }
}
