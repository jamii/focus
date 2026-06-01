use crate::app::{App, EditorId, IO, InputEvent, WindowId};
use crate::drawing::{Drawing, Rect};
use crate::style;

pub struct Window {
    pub editor_id: EditorId,
}

impl Window {
    pub(crate) fn new(editor_id: EditorId) -> Window {
        Window {
            editor_id: editor_id,
        }
    }
}

impl WindowId {
    pub fn assert_invariants(self, _app: &App) {}

    pub fn input(self, app: &App, io: &mut dyn IO, event: InputEvent) {
        let editor_id = self.get(app).editor_id;
        editor_id.input(app, io, event);
    }

    pub fn tick(self, app: &App, io: &mut dyn IO) {
        let editor_id = self.get(app).editor_id;
        editor_id.tick(app, io);
    }

    pub fn draw(self, app: &App, drawing: &mut Drawing) {
        let editor_id = self.get(app).editor_id;
        drawing.draw_rect(
            &app.atlas,
            Rect {
                pos: [0.0, 0.0],
                size: [1e9, 1e9],
            },
            style::BACKGROUND_COLOR,
        );
        editor_id.draw(app, drawing);
    }
}
