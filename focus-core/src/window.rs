use crate::app::{App, IO};
use crate::drawing::{Drawing, Rect};
use crate::editor::EditorId;
use crate::input::InputEvent;
use crate::style;

#[derive(PartialEq, Eq, Hash, Clone, Copy, Debug)]
pub struct WindowId(pub usize);

pub struct Window {
    pub(crate) editor_id: EditorId,
}

impl Window {
    pub(crate) fn new(editor_id: EditorId) -> Window {
        Window {
            editor_id: editor_id,
        }
    }
}

impl WindowId {
    pub(crate) fn get<'a>(self, app: &'a App) -> &'a Window {
        app.windows.get(&self).unwrap()
    }

    pub(crate) fn assert_invariants(self, _app: &App) {}

    pub(crate) fn input(self, app: &mut App, io: &mut dyn IO, event: InputEvent<'_>) {
        let editor_id = self.get(app).editor_id;
        editor_id.input(app, io, event);
    }

    pub(crate) fn tick(self, app: &mut App, io: &mut dyn IO) {
        let editor_id = self.get(app).editor_id;
        editor_id.tick(app, io);
    }

    pub(crate) fn draw(self, app: &mut App, drawing: &mut Drawing) {
        let editor_id = self.get(app).editor_id;
        drawing.draw_rect(
            Rect {
                pos: [0.0, 0.0],
                size: [1e9, 1e9],
            },
            style::BACKGROUND_COLOR,
        );
        editor_id.draw(app, drawing);
    }
}
