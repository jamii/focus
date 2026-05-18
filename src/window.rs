use crate::app::{App, EditorId, IO, InputEvent};
use crate::text::Drawing;

pub struct Window {
    pub editor_id: EditorId,
}

impl Window {
    pub fn new(editor_id: EditorId) -> Window {
        Window {
            editor_id: editor_id,
        }
    }

    pub fn input(&mut self, app: &App, io: &mut dyn IO, event: InputEvent) {
        app.get_editor_mut(self.editor_id).input(app, io, event);
    }

    pub fn tick(&mut self, app: &App, io: &mut dyn IO, redraw: &mut bool) {
        app.get_editor_mut(self.editor_id).tick(app, io, redraw);
    }

    pub fn draw(&self, app: &App, drawing: &mut Drawing) {
        app.get_editor(self.editor_id).draw(app, drawing);
    }
}
