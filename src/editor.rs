use winit::{
    event::ElementState,
    keyboard::{Key, NamedKey},
};

use crate::{
    app::{App, DocumentId, IO, InputEvent},
    document::{Edit, EditKind},
    style,
    text::{Drawing, Rect},
};

pub struct Editor {
    pub document_id: DocumentId,
    cursors: Vec<Cursor>,
    show_cursor: bool,
}

struct Cursor {
    pos: usize,
}

impl Editor {
    pub fn new(document_id: DocumentId) -> Self {
        Editor {
            document_id: document_id,
            cursors: vec![Cursor { pos: 0 }],
            show_cursor: true,
        }
    }

    pub fn input(&mut self, app: &App, _io: &mut dyn IO, event: InputEvent) {
        let mut document = self.document_id.get_mut(app);
        match event {
            InputEvent::KeyboardInput {
                event: key_event, ..
            } if key_event.state == ElementState::Pressed
                && !app.modifiers.state().control_key()
                && !app.modifiers.state().alt_key() =>
            {
                match key_event.logical_key.as_ref() {
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
                    Key::Named(NamedKey::Enter) => {
                        document.queue_edits(
                            self.cursors
                                .iter()
                                .map(|c| Edit {
                                    kind: EditKind::Insert,
                                    pos: c.pos,
                                    text: '\n'.to_string().into(),
                                })
                                .collect(),
                        );
                    }
                    Key::Named(NamedKey::Space) => {
                        document.queue_edits(
                            self.cursors
                                .iter()
                                .map(|c| Edit {
                                    kind: EditKind::Insert,
                                    pos: c.pos,
                                    text: ' '.to_string().into(),
                                })
                                .collect(),
                        );
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    pub fn tick(&mut self, _app: &App, io: &mut dyn IO, redraw: &mut bool) {
        let show_cursor = ((io.frame_start().as_millis() / 500) % 2) == 0;
        if self.show_cursor != show_cursor {
            self.show_cursor = show_cursor;
            *redraw = true;
        }
    }

    pub fn draw(&self, app: &App, drawing: &mut Drawing) {
        let document = self.document_id.get(app);
        document.draw(app, drawing);

        if self.show_cursor {
            for cursor in &self.cursors {
                let mut pos = app
                    .atlas
                    .screen_from_grid(document.grid_from_pos(cursor.pos));
                let mut size = [app.atlas.cell_size[0] as f32, app.atlas.cell_size[1] as f32];
                size[0] /= 8.0;
                pos[0] -= size[0] / 2.0;
                drawing.draw_rect(&app.atlas, Rect { pos, size }, style::TEXT_COLOR);
            }
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
