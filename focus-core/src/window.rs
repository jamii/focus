use crate::app::{App, INITIAL_SIZE, INITIAL_TITLE, IO};
use crate::buffer::BufferId;
use crate::drawing::Drawing;
use crate::editor;
use crate::input::{ButtonState, InputEvent, Key};
use crate::map::Map;
use crate::page::{self, PageId};

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy, Debug)]
pub struct WindowId(pub usize);

pub struct Windows {
    pub(crate) window_count: usize,
    pub(crate) open_count: usize,

    pub(crate) page_id: Map<WindowId, PageId>,
    pub(crate) open: Map<WindowId, bool>,
}

impl Windows {
    pub(crate) fn new() -> Windows {
        Windows {
            window_count: 0,
            open_count: 0,
            page_id: Map::new(),
            open: Map::new(),
        }
    }
}

pub(crate) fn new(app: &mut App, page_id: PageId) -> WindowId {
    let window_id = WindowId(app.windows.window_count);
    app.windows.window_count += 1;
    app.windows.open_count += 1;
    app.windows.page_id.insert(window_id, page_id);
    app.windows.open.insert(window_id, true);
    window_id
}

pub(crate) fn open(app: &mut App, io: &mut dyn IO, page_id: PageId) -> WindowId {
    let window_id = new(app, page_id);
    io.open_window(window_id, INITIAL_TITLE.to_string(), INITIAL_SIZE);
    window_id
}

pub fn open_scratch(app: &mut App, io: &mut dyn IO) -> WindowId {
    let editor_id = editor::new_scratch(app);
    let page_id = page::new_edit(app, editor_id);
    open(app, io, page_id)
}

pub fn open_edit(app: &mut App, io: &mut dyn IO, buffer_id: BufferId) -> WindowId {
    let editor_id = editor::new(app, buffer_id);
    let page_id = page::new_edit(app, editor_id);
    open(app, io, page_id)
}

pub(crate) fn assert_invariants(app: &App) {
    let windows = &app.windows;
    assert_eq!(windows.page_id.len(), windows.window_count);
    assert_eq!(windows.open.len(), windows.window_count);
    assert_eq!(
        windows.open.values().filter(|open| **open).count(),
        windows.open_count
    );
    for window_id in windows.page_id.keys() {
        let page_id = windows.page_id[window_id];
        assert!(page_id.0 < app.pages.page_count);
    }
}

impl WindowId {
    pub(crate) fn tick(self, app: &mut App, io: &mut dyn IO) {
        let page_id = app.windows.page_id[self];
        page_id.tick(app, io);
    }

    pub(crate) fn input(self, app: &mut App, io: &mut dyn IO, event: InputEvent<'_>) {
        assert!(app.windows.open[self], "input for closed window {:?}", self,);

        let handled = match &event {
            InputEvent::CloseRequested => {
                self.close(app, io);
                true
            }
            InputEvent::Key {
                state, logical_key, ..
            } if *state == ButtonState::Pressed && app.modifiers.control && !app.modifiers.alt => {
                match *logical_key {
                    Key::Character("n") => {
                        open_scratch(app, io);
                        true
                    }
                    Key::Character("m") => {
                        let page_id = app.windows.page_id[self];
                        let editor_id = app.pages.editor_ids[page_id][0];
                        let buffer_id = app.editors.buffer_id[editor_id];
                        open_edit(app, io, buffer_id);
                        true
                    }
                    Key::Character("o") => {
                        let page_id = page::new_open_file(app, io);
                        app.windows.page_id[self] = page_id;
                        true
                    }
                    _ => false,
                }
            }
            _ => false,
        };

        if !handled {
            let page_id = app.windows.page_id[self];
            page_id.input(app, io, self, event);
        }
    }

    fn close(self, app: &mut App, io: &mut dyn IO) {
        assert!(
            app.windows.open[self],
            "tried to close window {:?}, but it is not open",
            self,
        );

        let page_id = app.windows.page_id[self];
        page_id.input(app, io, self, InputEvent::FocusChanged { focused: false });

        app.windows.open[self] = false;
        app.windows.open_count -= 1;
        io.close_window(self);
        if app.windows.open_count == 0 {
            io.exit();
        }
    }

    pub(crate) fn draw(self, app: &mut App, drawing: &mut Drawing) {
        let page_id = app.windows.page_id[self];
        page_id.draw(app, drawing);
    }
}
