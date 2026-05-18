use crate::app::{App, EditorId};
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

    pub fn draw(&self, app: &App, drawing: &mut Drawing) {
        app.get_editor(self.editor_id).draw(app, drawing);
    }
}
