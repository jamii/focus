use crate::app::{App, EditorId, IO, InputEvent, WindowId};
use crate::drawing::{Drawing, Rect};
use crate::style;

pub struct Window {
    pub editor_id: EditorId,
}

impl Window {
    pub fn assert_invariants(&self) {}

    pub(crate) fn new(editor_id: EditorId) -> Window {
        Window {
            editor_id: editor_id,
        }
    }

    pub fn input(window_id: WindowId, app: &App, io: &mut dyn IO, event: InputEvent) {
        let editor_id = window_id.get(app).editor_id;
        crate::editor::Editor::input(editor_id, app, io, event);
    }

    pub fn tick(window_id: WindowId, app: &App, io: &mut dyn IO) {
        let editor_id = window_id.get(app).editor_id;
        crate::editor::Editor::tick(editor_id, app, io);
    }

    pub fn draw(window_id: WindowId, app: &App, drawing: &mut Drawing) {
        let editor_id = window_id.get(app).editor_id;
        drawing.draw_rect(
            &app.atlas,
            Rect {
                pos: [0.0, 0.0],
                size: [1e9, 1e9],
            },
            style::BACKGROUND_COLOR,
        );
        crate::editor::Editor::draw(editor_id, app, drawing);
    }
}
