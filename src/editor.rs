use winit::event::ElementState;

use crate::{
    app::{App, DocumentId, IO, InputEvent},
    document::{Edit, EditKind},
    text::Drawing,
};

pub struct Editor {
    pub document_id: DocumentId,
    cursors: Vec<Cursor>,
}

struct Cursor {
    pos: usize,
}

impl Editor {
    pub fn new(document_id: DocumentId) -> Self {
        Editor {
            document_id: document_id,
            cursors: vec![Cursor { pos: 0 }],
        }
    }

    pub fn input(&mut self, app: &App, _io: &mut dyn IO, event: InputEvent) {
        let mut document = app.get_document_mut(self.document_id);
        match event {
            InputEvent::KeyboardInput {
                event: key_event, ..
            } if key_event.state == ElementState::Pressed => match key_event.text.as_ref() {
                Some(char) => {
                    let end = document.text.len();
                    document.queue_edits(vec![Edit {
                        kind: EditKind::Insert,
                        pos: end,
                        text: char.as_ref().into(),
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

    pub fn handle_edits(&mut self, _edits: &[Edit]) {}
}
