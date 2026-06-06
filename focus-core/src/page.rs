use crate::{
    app::{App, IO},
    document::DocumentId,
    drawing::Drawing,
    editor::EditorId,
    input::InputEvent,
};

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy, Debug)]
pub struct PageId(pub(crate) usize);

pub enum Page {
    Single { editor_id: EditorId },
}

impl PageId {
    pub(crate) fn get<'a>(self, app: &'a App) -> &'a Page {
        app.pages.get(&self).unwrap()
    }

    pub(crate) fn get_mut<'a>(self, app: &'a mut App) -> &'a mut Page {
        app.pages.get_mut(&self).unwrap()
    }

    pub(crate) fn assert_invariants(self, _app: &App) {}

    pub(crate) fn document_id(self, app: &App) -> DocumentId {
        match self.get(app) {
            Page::Single { editor_id } => editor_id.get(app).document_id,
        }
    }

    pub(crate) fn tick(self, app: &mut App, io: &mut dyn IO) {
        match self.get(app) {
            Page::Single { editor_id } => editor_id.tick(app, io),
        }
    }

    pub(crate) fn input(self, app: &mut App, io: &mut dyn IO, event: InputEvent<'_>) {
        match self.get(app) {
            Page::Single { editor_id } => editor_id.input(app, io, event),
        }
    }

    pub(crate) fn draw(self, app: &mut App, drawing: &mut Drawing) {
        match self.get(app) {
            Page::Single { editor_id } => editor_id.draw(app, drawing),
        }
    }
}
