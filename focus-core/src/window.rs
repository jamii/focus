use crate::app::{App, IO};
use crate::drawing::Drawing;
use crate::input::{ButtonState, InputEvent, Key};
use crate::page::{Page, PageId};

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

    pub(crate) fn get_mut<'a>(self, app: &'a mut App) -> &'a mut Window {
        app.windows.get_mut(&self).unwrap()
    }

    pub(crate) fn tick(self, app: &mut App, io: &mut dyn IO) {
        let page_id = self.get(app).page_id;
        page_id.tick(app, io);
    }

    pub(crate) fn input(self, app: &mut App, io: &mut dyn IO, event: InputEvent<'_>) {
        let handled = match &event {
            InputEvent::Key {
                state, logical_key, ..
            } if *state == ButtonState::Pressed && app.modifiers.control && !app.modifiers.alt => {
                match *logical_key {
                    Key::Character("n") => {
                        app.insert_window_empty(io);
                        true
                    }
                    Key::Character("o") => {
                        let page = Page::new_file_opener(app);
                        self.get_mut(app).page_id = app.insert_page(page);
                        true
                    }
                    _ => false,
                }
            }
            _ => false,
        };

        if !handled {
            let page_id = self.get(app).page_id;
            page_id.input(app, io, event);
        }
    }

    pub(crate) fn draw(self, app: &mut App, drawing: &mut Drawing) {
        self.get(app).page_id.draw(app, drawing);
    }
}
