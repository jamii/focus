use crate::app::{App, EditorId, IO, InputEvent};
use crate::drawing::{Drawing, Rect};
use crate::style;

pub struct Window {
    pub editor_id: EditorId,
}

impl Window {
    pub fn assert_invariants(&self) {}

    pub fn new(editor_id: EditorId) -> Window {
        Window {
            editor_id: editor_id,
        }
    }

    pub fn input(&mut self, app: &App, io: &mut dyn IO, event: InputEvent) {
        self.editor_id.get_mut(app).input(app, io, event);
    }

    pub fn tick(&mut self, app: &App, io: &mut dyn IO) {
        self.editor_id.get_mut(app).tick(app, io);
    }

    pub fn draw(&mut self, app: &App, drawing: &mut Drawing) {
        drawing.draw_rect(
            &app.atlas,
            Rect {
                pos: [0.0, 0.0],
                size: [1e9, 1e9],
            },
            style::BACKGROUND_COLOR,
        );
        self.editor_id.get_mut(app).draw(app, drawing);
    }
}
