use bstr::BStr;
use std::os::unix::ffi::OsStrExt;

use crate::{
    app::{App, IO},
    buffer::{Source, SourceFile},
    drawing::{Drawing, Rect},
    editor::EditorId,
    input::{ButtonState, InputEvent},
    style::{BACKGROUND_COLOR, HIGHLIGHT_COLOR},
};

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy, Debug)]
pub struct PageId(pub(crate) usize);

pub struct Page {
    content: PageContent,
    editor_ids: Vec<EditorId>,
    editor_rects: Vec<Rect>,
    focus: usize, // index into editor_ids
    dragging: bool,
    last_draw_size: [f32; 2],
}
pub enum PageContent {
    Edit { cursor_main_grid: [usize; 2] },
    OpenFile,
}

pub(crate) const GAP: f32 = 1.0;

impl Page {
    pub(crate) fn new_edit(app: &mut App, editor_id: EditorId) -> Page {
        let status_bar_id = app.insert_editor_empty();
        Page {
            content: PageContent::Edit {
                // Impossible grid, so the first tick always updates the status bar.
                cursor_main_grid: [usize::MAX, usize::MAX],
            },
            editor_ids: vec![editor_id, status_bar_id],
            editor_rects: vec![
                Rect {
                    pos: [0.0, 0.0],
                    size: [0.0, 0.0]
                };
                2
            ],
            focus: 0,
            dragging: false,
            last_draw_size: [0.0, 0.0],
        }
    }

    pub(crate) fn new_open_file(app: &mut App) -> Page {
        let preview_id = app.insert_editor_empty();
        let path_id = app.insert_editor_empty();
        let list_id = app.insert_editor_empty();
        Page {
            content: PageContent::OpenFile,
            editor_ids: vec![preview_id, path_id, list_id],
            editor_rects: vec![
                Rect {
                    pos: [0.0, 0.0],
                    size: [0.0, 0.0]
                };
                3
            ],
            focus: 1,
            dragging: false,
            last_draw_size: [0.0, 0.0],
        }
    }

    pub fn assert_invariants(&self) {
        match self.content {
            PageContent::Edit { .. } => assert!(self.editor_ids.len() == 2),
            PageContent::OpenFile => assert!(self.editor_ids.len() == 3),
        }
        assert!(self.editor_rects.len() == self.editor_ids.len());
        for rect in &self.editor_rects {
            assert!(rect.pos[0] + rect.size[0] <= self.last_draw_size[0]);
            assert!(rect.pos[1] + rect.size[1] <= self.last_draw_size[1]);
        }
        for (ix0, rect0) in self.editor_rects.iter().enumerate() {
            for (ix1, rect1) in self.editor_rects.iter().enumerate() {
                if ix0 != ix1 {
                    let overlap = Rect::intersect(*rect0, *rect1).size;
                    assert!(overlap[0] == 0.0 || overlap[1] == 0.0);
                }
            }
        }
        assert!(self.focus < self.editor_ids.len());
    }

    pub(crate) fn editor_ids(&self) -> &[EditorId] {
        &*self.editor_ids
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

    pub(crate) fn tick(self, app: &mut App) {
        let editor_ids = self.get(app).editor_ids.clone();
        for editor_id in &editor_ids {
            editor_id.tick(app);
        }

        match self.get(app).content {
            PageContent::Edit { cursor_main_grid } => {
                let &[editor_id, status_bar_id] = &*self.get(app).editor_ids else {
                    unreachable!()
                };

                // If cursor has moved, update status bar text.
                let cursor_main_offset = editor_id.get(app).cursors.last().unwrap().head.offset;
                let cursor_main_grid_new = editor_id
                    .get(app)
                    .buffer_id
                    .grid_from_offset(app, cursor_main_offset);
                if cursor_main_grid_new != cursor_main_grid {
                    let PageContent::Edit { cursor_main_grid } = &mut self.get_mut(app).content
                    else {
                        unreachable!()
                    };
                    *cursor_main_grid = cursor_main_grid_new;

                    let status_bar_buffer_id = status_bar_id.get(app).buffer_id;
                    let source = match editor_id.get(app).buffer_id.source(app) {
                        Source::Scratch => BStr::new("scratch"),
                        Source::File(SourceFile { absolute_path, .. }) => {
                            BStr::new(absolute_path.as_os_str().as_bytes())
                        }
                    };
                    let status_text = format!(
                        "{}:{}:{}",
                        source,
                        cursor_main_grid_new[1] + 1,
                        cursor_main_grid_new[0] + 1
                    );
                    status_bar_buffer_id.replace(app, BStr::new(status_text.as_bytes()));
                    // The status bar editor already ticked this frame, so catch
                    // it up now rather than leaving it stale when App::tick
                    // trims the diff log.
                    status_bar_id.catch_up(app);
                }
            }
            PageContent::OpenFile => {
                // TODO Cache the path buffer's version and reload the lister
                // when it changes.
            }
        }
    }

    pub(crate) fn input(self, app: &mut App, io: &mut dyn IO, mut event: InputEvent<'_>) {
        let page = self.get_mut(app);

        // Update drag state.
        match event {
            InputEvent::MouseButton { state, .. } => {
                page.dragging = state == ButtonState::Pressed;
            }
            _ => {}
        }

        // Check if focus changed.
        match event {
            InputEvent::MouseMoved { position } => {
                if !page.dragging {
                    let focus_new = (0..page.editor_rects.len())
                        .into_iter()
                        .filter(|i| page.editor_rects[*i].contains(position))
                        .next()
                        .unwrap_or(page.focus);
                    if page.focus != focus_new {
                        page.focus = focus_new;
                        let editor_ids = page.editor_ids.clone();
                        for (i, editor_id) in editor_ids.iter().enumerate() {
                            editor_id.input(
                                app,
                                io,
                                InputEvent::FocusChanged {
                                    focused: focus_new == i,
                                },
                            );
                        }
                    }
                }
            }
            _ => {}
        }

        let page = self.get(app);

        // Adjust mouse event positions to be relative to the focused editor.
        match &mut event {
            InputEvent::MouseMoved { position } | InputEvent::MouseButton { position, .. } => {
                position[0] -= page.editor_rects[page.focus].pos[0];
                position[1] -= page.editor_rects[page.focus].pos[1];
            }
            _ => {}
        }

        // Dispatch event to focused editor.
        page.editor_ids[page.focus].input(app, io, event);
    }

    pub(crate) fn draw(self, app: &mut App, drawing: &mut Drawing) {
        let cell_size = app.cell_size();
        let page = self.get_mut(app);

        // Resize if necessary.
        if page.last_draw_size != drawing.size() {
            page.last_draw_size = drawing.size();
            let page_rect = Rect {
                pos: [0.0, 0.0],
                size: page.last_draw_size,
            };
            let editor_rects = match page.content {
                PageContent::Edit { .. } => {
                    let [editor_rect, status_bar_rect] =
                        page_rect.split_from_bottom(cell_size[1] as f32, GAP);
                    vec![editor_rect, status_bar_rect]
                }
                PageContent::OpenFile => {
                    let [preview_rect, rest] =
                        page_rect.split_from_bottom(page_rect.size[1] as f32 / 2.0, GAP);
                    let [path_rect, list_rect] = rest.split_from_top(cell_size[1] as f32, GAP);
                    vec![preview_rect, path_rect, list_rect]
                }
            };
            // The splits saturate, so the layout can never stick out of the
            // page, even when the page is smaller than the layout wants.
            for rect in &editor_rects {
                for i in 0..2 {
                    assert!(
                        rect.size[i] >= 0.0
                            && rect.pos[i] >= 0.0
                            && rect.pos[i] + rect.size[i] <= page_rect.size[i],
                        "editor rect {:?} sticks out of page {:?}",
                        rect,
                        page_rect,
                    );
                }
            }
            page.editor_rects = editor_rects;
        }

        // Draw gaps.
        drawing.draw_rect(
            Rect {
                pos: [0.0, 0.0],
                size: page.last_draw_size,
            },
            BACKGROUND_COLOR,
        );
        drawing.draw_rect(
            Rect {
                pos: [0.0, 0.0],
                size: page.last_draw_size,
            },
            HIGHLIGHT_COLOR,
        );

        // Draw each editor.
        let editor_ids = page.editor_ids.clone();
        let editor_rects = page.editor_rects.clone();
        let focus = page.focus;
        for (i, (editor_id, editor_rect)) in editor_ids.iter().zip(editor_rects.iter()).enumerate()
        {
            let mut drawing = drawing.push_clip_rect(*editor_rect);
            editor_id.draw(app, &mut drawing, focus == i);
        }
    }
}
