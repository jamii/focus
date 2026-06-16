use crate::app::{App, IO};
use crate::drawing::Drawing;
use crate::input::InputEvent;
use crate::page::PageId;

#[derive(PartialEq, Eq, Hash, Clone, Copy, Debug)]
pub struct WindowId(pub usize);

pub struct Window {
    pub(crate) page_id: PageId,
}

impl Window {
    pub(crate) fn new(page_id: PageId) -> Window {
        Window { page_id }
    }

    pub(crate) fn assert_invariants(&self) {}
}

impl WindowId {
    pub(crate) fn get<'a>(self, app: &'a App) -> &'a Window {
        app.windows.get(&self).unwrap()
    }

    pub(crate) fn tick(self, app: &mut App, io: &mut dyn IO) {
        let page_id = self.get(app).page_id;
        page_id.tick(app, io);
    }

    pub(crate) fn input(self, app: &mut App, io: &mut dyn IO, event: InputEvent<'_>) {
        let page_id = self.get(app).page_id;
        page_id.input(app, io, event);
    }

    pub(crate) fn draw(self, app: &mut App, drawing: &mut Drawing) {
        self.get(app).page_id.draw(app, drawing);
    }
}
