use winit::{event::ElementState, keyboard::Key};

use crate::{
    app::{App, DocumentId, IO, InputEvent},
    document::{Edit, EditKind},
    style,
    text::{Drawing, Rect},
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
            } if key_event.state == ElementState::Pressed => match key_event.logical_key.as_ref() {
                Key::Character(char) => {
                    document.queue_edits(
                        self.cursors
                            .iter()
                            .map(|c| Edit {
                                kind: EditKind::Insert,
                                pos: c.pos,
                                text: char.into(),
                            })
                            .collect(),
                    );
                }
                _ => {}
            },
            _ => {}
        }
    }

    pub fn draw(&self, app: &App, drawing: &mut Drawing) {
        app.get_document(self.document_id).draw(app, drawing);

        for cursor in &self.cursors {
            let mut pos = app.atlas.grid_to_screen([cursor.pos, 0]);
            let mut size = [app.atlas.cell_size[0] as f32, app.atlas.cell_size[1] as f32];
            size[0] /= 8.0;
            pos[0] -= size[0] / 2.0;
            drawing.draw_rect(&app.atlas, Rect { pos, size }, style::TEXT_COLOR);
        }
    }

    pub fn handle_edits(&mut self, edits: &[Edit]) {
        for cursor in &mut self.cursors {
            let mut pos_diff = 0;
            for edit in edits.iter() {
                if cursor.pos < edit.pos {
                    break;
                }
                match edit.kind {
                    EditKind::Insert => {
                        pos_diff += edit.text.len();
                    }
                    EditKind::Delete => {
                        pos_diff -= edit.text.len();
                    }
                }
            }
            cursor.pos += pos_diff;
        }
    }
}
