use bstr::BStr;
use std::os::unix::ffi::OsStrExt;

use crate::{
    app::{App, IO},
    document::{DocumentId, Source, SourceFile},
    drawing::{Drawing, Rect},
    editor::EditorId,
    input::{ButtonState, InputEvent},
};

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy, Debug)]
pub struct PageId(pub(crate) usize);

pub struct Page {
    content: PageContent,
    dragging: bool,
    last_draw_size: [f32; 2],
}
pub enum PageContent {
    Single {
        editor_id: EditorId,
        status_bar_id: EditorId,
        focus: PageSingleFocus,
    },
}

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum PageSingleFocus {
    Editor,
    StatusBar,
}

impl Page {
    pub(crate) fn new_single(editor_id: EditorId, app: &mut App) -> Page {
        let status_bar_id = app.insert_editor_empty();
        Page {
            content: PageContent::Single {
                editor_id,
                status_bar_id,
                focus: PageSingleFocus::Editor,
            },
            dragging: false,
            last_draw_size: [0.0, 0.0],
        }
    }
}

impl PageId {
    pub(crate) fn get<'a>(self, app: &'a App) -> &'a Page {
        app.pages.get(&self).unwrap()
    }

    #[allow(dead_code)]
    pub(crate) fn get_mut<'a>(self, app: &'a mut App) -> &'a mut Page {
        app.pages.get_mut(&self).unwrap()
    }

    pub(crate) fn assert_invariants(self, _app: &App) {}

    pub(crate) fn document_id(self, app: &App) -> DocumentId {
        match self.get(app).content {
            PageContent::Single { editor_id, .. } => editor_id.get(app).document_id,
        }
    }

    pub(crate) fn tick(self, app: &mut App, io: &mut dyn IO) {
        match self.get(app).content {
            PageContent::Single {
                editor_id,
                status_bar_id,
                ..
            } => {
                editor_id.tick(app, io);

                let status_bar_document_id = status_bar_id.get(app).document_id;
                let cursor_main_offset = editor_id.get(app).cursors.last().unwrap().head.offset;
                let grid = editor_id.grid_from_offset(app, cursor_main_offset);
                let source = match editor_id.get(app).document_id.source(app) {
                    Source::Scratch => BStr::new("scratch"),
                    Source::File(SourceFile { absolute_path, .. }) => {
                        BStr::new(absolute_path.as_os_str().as_bytes())
                    }
                };
                let status_text = format!("{} {}:{}", source, grid[0][1] + 1, grid[0][0] + 1);
                status_bar_document_id.replace(app, BStr::new(status_text.as_bytes()));

                status_bar_id.tick(app, io);
            }
        }
    }

    pub(crate) fn input(self, app: &mut App, io: &mut dyn IO, mut event: InputEvent<'_>) {
        match event {
            InputEvent::MouseButton { state, .. } => {
                self.get_mut(app).dragging = state == ButtonState::Pressed;
            }
            _ => {}
        }

        let cell_size = app.cell_size();
        match self.get_mut(app) {
            &mut Page {
                content:
                    PageContent::Single {
                        editor_id,
                        status_bar_id,
                        ref mut focus,
                        ..
                    },
                dragging,
                last_draw_size: last_size,
                ..
            } => match event {
                InputEvent::MouseMoved { position } => {
                    if !dragging {
                        let focus_new = if position[1] > last_size[1] - (cell_size[1] as f32) {
                            PageSingleFocus::StatusBar
                        } else {
                            PageSingleFocus::Editor
                        };
                        if *focus != focus_new {
                            *focus = focus_new;
                            editor_id.input(
                                app,
                                io,
                                InputEvent::FocusChanged {
                                    focused: focus_new == PageSingleFocus::Editor,
                                },
                            );
                            status_bar_id.input(
                                app,
                                io,
                                InputEvent::FocusChanged {
                                    focused: focus_new == PageSingleFocus::StatusBar,
                                },
                            );
                        }
                    }
                }
                _ => {}
            },
        }

        match self.get_mut(app) {
            &mut Page {
                content: PageContent::Single { focus, .. },
                last_draw_size: last_size,
                ..
            } => match &mut event {
                InputEvent::MouseMoved { position } | InputEvent::MouseButton { position, .. } => {
                    match focus {
                        PageSingleFocus::Editor => {}
                        PageSingleFocus::StatusBar => {
                            position[1] -= last_size[1] - (cell_size[1] as f32)
                        }
                    }
                }
                _ => {}
            },
        }

        match self.get_mut(app) {
            Page {
                content:
                    PageContent::Single {
                        editor_id,
                        status_bar_id,
                        focus,
                        ..
                    },
                ..
            } => {
                let id = match focus {
                    PageSingleFocus::Editor => editor_id,
                    PageSingleFocus::StatusBar => status_bar_id,
                };
                id.input(app, io, event);
            }
        }
    }

    pub(crate) fn draw(self, app: &mut App, drawing: &mut Drawing) {
        self.get_mut(app).last_draw_size = drawing.size();

        match self.get(app).content {
            PageContent::Single {
                editor_id,
                status_bar_id,
                focus,
            } => {
                let status_bar_size = [drawing.size()[0], app.cell_size()[1] as f32];
                let mut editor_size = drawing.size();
                editor_size[1] -= status_bar_size[1];

                {
                    let mut drawing = drawing.push_clip_rect(Rect {
                        pos: [0.0, 0.0],
                        size: editor_size,
                    });
                    editor_id.draw(app, &mut drawing, focus == PageSingleFocus::Editor);
                }

                {
                    let mut drawing = drawing.push_clip_rect(Rect {
                        pos: [0.0, editor_size[1]],
                        size: status_bar_size,
                    });
                    status_bar_id.draw(app, &mut drawing, focus == PageSingleFocus::StatusBar);
                }
            }
        }
    }
}
