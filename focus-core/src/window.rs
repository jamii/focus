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

    pub(crate) page_stack: Map<WindowId, Vec<PageId>>,
    pub(crate) open: Map<WindowId, bool>,
}

impl Windows {
    pub(crate) fn new() -> Windows {
        Windows {
            window_count: 0,
            open_count: 0,
            page_stack: Map::new(),
            open: Map::new(),
        }
    }
}

pub(crate) fn new(app: &mut App, page_id: PageId) -> WindowId {
    let window_id = WindowId(app.windows.window_count);
    app.windows.window_count += 1;
    app.windows.open_count += 1;
    app.windows.page_stack.insert(window_id, vec![page_id]);
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
    assert_eq!(windows.page_stack.len(), windows.window_count);
    assert_eq!(windows.open.len(), windows.window_count);
    assert_eq!(
        windows.open.values().filter(|open| **open).count(),
        windows.open_count
    );
    for window_id in windows.page_stack.keys() {
        let page_stack = &windows.page_stack[window_id];
        assert!(
            !page_stack.is_empty(),
            "window {:?} has an empty page stack",
            window_id
        );
        for page_id in page_stack {
            assert!(page_id.0 < app.pages.page_count);
        }
    }
}

impl WindowId {
    fn current_page(self, app: &App) -> PageId {
        *app.windows.page_stack[self]
            .last()
            .unwrap_or_else(|| panic!("window {:?} has an empty page stack", self))
    }

    pub(crate) fn push_page(self, app: &mut App, io: &mut dyn IO, page_id: PageId) {
        assert!(
            app.windows.open[self],
            "tried to push page for closed window {:?}",
            self,
        );

        let old_page_id = self.current_page(app);
        old_page_id.input(app, io, self, InputEvent::FocusChanged { focused: false });
        app.windows.page_stack[self].push(page_id);
        page_id.input(app, io, self, InputEvent::FocusChanged { focused: true });
    }

    pub(crate) fn replace_page(self, app: &mut App, io: &mut dyn IO, page_id: PageId) {
        assert!(
            app.windows.open[self],
            "tried to replace page for closed window {:?}",
            self,
        );

        let old_page_id = app.windows.page_stack[self]
            .pop()
            .unwrap_or_else(|| panic!("window {:?} has an empty page stack", self));
        old_page_id.input(app, io, self, InputEvent::FocusChanged { focused: false });
        old_page_id.teardown(app, io);
        app.windows.page_stack[self].push(page_id);
        page_id.input(app, io, self, InputEvent::FocusChanged { focused: true });
    }

    fn pop_page(self, app: &mut App, io: &mut dyn IO) {
        assert!(
            app.windows.open[self],
            "tried to pop page for closed window {:?}",
            self,
        );

        let page_id = app.windows.page_stack[self]
            .pop()
            .unwrap_or_else(|| panic!("window {:?} has an empty page stack", self));
        page_id.input(app, io, self, InputEvent::FocusChanged { focused: false });
        page_id.teardown(app, io);

        if app.windows.page_stack[self].is_empty() {
            let editor_id = editor::new_scratch(app);
            let page_id = page::new_edit(app, editor_id);
            app.windows.page_stack[self].push(page_id);
        }

        let page_id = self.current_page(app);
        page_id.input(app, io, self, InputEvent::FocusChanged { focused: true });
    }

    pub(crate) fn tick(self, app: &mut App, io: &mut dyn IO) {
        let page_stack = app.windows.page_stack[self].clone();
        let (page_id, hidden) = page_stack.split_last().unwrap();
        for page_id in hidden {
            page_id.tick_background(app, io);
        }
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
                    Key::Character("q") => {
                        self.pop_page(app, io);
                        true
                    }
                    Key::Character("n") => {
                        // Open a new window on the same buffer (was Ctrl+m).
                        let page_id = self.current_page(app);
                        let editor_id = app.pages.editor_ids[page_id][0];
                        let buffer_id = app.editors.buffer_id[editor_id];
                        open_edit(app, io, buffer_id);
                        true
                    }
                    Key::Character("m") => {
                        // Open the runner dir picker.
                        let page_id = self.current_page(app);
                        let dir = page_id
                            .current_path(app)
                            .and_then(|path| path.parent().map(|p| p.to_path_buf()))
                            .unwrap_or_else(|| io.current_dir());
                        let root = io.repo_root(&dir);
                        let dir_page_id = page::new_choose_dir(app, io, root);
                        self.push_page(app, io, dir_page_id);
                        true
                    }
                    Key::Character("o") => {
                        let page_id = self.current_page(app);
                        let dir = page_id
                            .current_path(app)
                            .and_then(|path| path.parent().map(|parent| parent.to_path_buf()))
                            .unwrap_or_else(|| io.current_dir());
                        let page_id = page::new_open_file(app, dir);
                        self.push_page(app, io, page_id);
                        true
                    }
                    Key::Character("p") => {
                        let page_id = self.current_page(app);
                        let dir = page_id
                            .current_path(app)
                            .and_then(|path| path.parent().map(|parent| parent.to_path_buf()))
                            .unwrap_or_else(|| io.current_dir());
                        let page_id = page::new_open_file_from_repo(app, io, dir);
                        self.push_page(app, io, page_id);
                        true
                    }
                    _ => false,
                }
            }
            InputEvent::Key {
                state, logical_key, ..
            } if *state == ButtonState::Pressed && !app.modifiers.control && app.modifiers.alt => {
                match *logical_key {
                    Key::Character("p") => {
                        let page_id = page::new_open_buffer(app);
                        self.push_page(app, io, page_id);
                        true
                    }
                    Key::Character("f") => {
                        let page_id = self.current_page(app);
                        let dir = page_id
                            .current_path(app)
                            .and_then(|path| path.parent().map(|parent| parent.to_path_buf()))
                            .unwrap_or_else(|| io.current_dir());
                        let page_id = page::new_search_repo(app, dir);
                        self.push_page(app, io, page_id);
                        true
                    }
                    _ => false,
                }
            }
            _ => false,
        };

        if !handled {
            let page_id = self.current_page(app);
            page_id.input(app, io, self, event);
        }
    }

    fn close(self, app: &mut App, io: &mut dyn IO) {
        assert!(
            app.windows.open[self],
            "tried to close window {:?}, but it is not open",
            self,
        );

        let page_id = self.current_page(app);
        page_id.input(app, io, self, InputEvent::FocusChanged { focused: false });

        app.windows.open[self] = false;
        app.windows.open_count -= 1;
        io.close_window(self);
        // The window will never show its pages again; tear them all down.
        let page_stack = app.windows.page_stack[self].clone();
        for page_id in page_stack {
            page_id.teardown(app, io);
        }
        if app.windows.open_count == 0 {
            io.exit();
        }
    }

    pub(crate) fn draw(self, app: &mut App, drawing: &mut Drawing) {
        let page_id = self.current_page(app);
        page_id.draw(app, drawing);
    }
}
