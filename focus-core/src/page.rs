use bstr::BStr;
use std::os::unix::ffi::OsStrExt;

use crate::{
    app::{App, IO},
    document::{Source, SourceFile},
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
    Single,
}

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum PageSingleFocus {
    Editor,
    StatusBar,
}

pub(crate) const GAP: f32 = 1.0;

impl Page {
    pub(crate) fn new_single(editor_id: EditorId, app: &mut App) -> Page {
        let status_bar_id = app.insert_editor_empty();
        Page {
            content: PageContent::Single,
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

    pub fn assert_invariants(&self) {
        match self.content {
            PageContent::Single => assert!(self.editor_ids.len() == 2),
        }
        assert!(self.editor_rects.len() == self.editor_ids.len());
        for rect in &self.editor_rects {
            assert!(rect.pos[0] + rect.size[0] <= self.last_draw_size[0]);
            assert!(rect.pos[1] + rect.size[1] <= self.last_draw_size[1]);
        }
        for (ix0, rect0) in self.editor_rects.iter().enumerate() {
            for (ix1, rect1) in self.editor_rects.iter().enumerate() {
                if ix0 != ix1 {
                    assert!(Rect::intersect(*rect0, *rect1).size == [0.0, 0.0]);
                }
            }
        }
        assert!(self.focus < self.editor_ids.len());
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

    pub(crate) fn tick(self, app: &mut App, io: &mut dyn IO) {
        match self.get(app).content {
            PageContent::Single => {
                let &[editor_id, status_bar_id] = &*self.get(app).editor_ids else {
                    unreachable!()
                };

                editor_id.tick(app, io);

                // Update status bar text.
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
            match page.content {
                PageContent::Single => {
                    page.editor_rects =
                        page_rect.split_from_bottom(cell_size[1] as f32, GAP).into();
                }
            }
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
