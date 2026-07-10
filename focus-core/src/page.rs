use bstr::BStr;
use std::os::unix::ffi::OsStrExt;

use crate::{
    app::{App, IO},
    buffer::{OffsetDiff, Source, SourceFile},
    drawing::{Drawing, Rect},
    editor::{self, EditorId},
    input::{ButtonState, InputEvent},
    map::{Map, MapKey},
    style::{BACKGROUND_COLOR, HIGHLIGHT_COLOR},
};

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy, Debug)]
pub struct PageId(pub(crate) usize);

impl MapKey for PageId {
    fn index(self) -> usize {
        self.0
    }

    fn from_index(index: usize) -> Self {
        PageId(index)
    }
}

pub struct Pages {
    pub(crate) page_count: usize,

    content: Map<PageId, PageContent>,
    pub(crate) editor_ids: Map<PageId, Vec<EditorId>>,
    editor_rects: Map<PageId, Vec<Rect>>,
    focus: Map<PageId, usize>,
    dragging: Map<PageId, bool>,
    last_draw_size: Map<PageId, [f32; 2]>,
}

#[derive(Clone, Copy)]
pub enum PageContent {
    Edit,
    OpenFile,
}

pub(crate) const GAP: f32 = 1.0;

impl Pages {
    pub(crate) fn new() -> Pages {
        Pages {
            page_count: 0,
            content: Map::new(),
            editor_ids: Map::new(),
            editor_rects: Map::new(),
            focus: Map::new(),
            dragging: Map::new(),
            last_draw_size: Map::new(),
        }
    }
}

pub(crate) fn new_edit(app: &mut App, editor_id: EditorId) -> PageId {
    let status_bar_id = editor::new_scratch(app);
    insert(app, PageContent::Edit, vec![editor_id, status_bar_id], 0)
}

pub(crate) fn new_open_file(app: &mut App) -> PageId {
    let preview_id = editor::new_scratch(app);
    let path_id = editor::new_scratch(app);
    let list_id = editor::new_scratch(app);
    insert(
        app,
        PageContent::OpenFile,
        vec![preview_id, path_id, list_id],
        1,
    )
}

fn insert(app: &mut App, content: PageContent, editor_ids: Vec<EditorId>, focus: usize) -> PageId {
    let page_id = PageId(app.pages.page_count);
    app.pages.page_count += 1;
    let rects = vec![
        Rect {
            pos: [0.0, 0.0],
            size: [0.0, 0.0]
        };
        editor_ids.len()
    ];
    app.pages.content.insert(page_id, content);
    app.pages.editor_ids.insert(page_id, editor_ids);
    app.pages.editor_rects.insert(page_id, rects);
    app.pages.focus.insert(page_id, focus);
    app.pages.dragging.insert(page_id, false);
    app.pages.last_draw_size.insert(page_id, [0.0, 0.0]);
    page_id
}

pub(crate) fn assert_invariants(app: &App) {
    let pages = &app.pages;
    assert_eq!(pages.content.len(), pages.page_count);
    assert_eq!(pages.editor_ids.len(), pages.page_count);
    assert_eq!(pages.editor_rects.len(), pages.page_count);
    assert_eq!(pages.focus.len(), pages.page_count);
    assert_eq!(pages.dragging.len(), pages.page_count);
    assert_eq!(pages.last_draw_size.len(), pages.page_count);
    for page_id in (0..pages.page_count).map(PageId) {
        let editor_ids = pages.editor_ids.get(page_id);
        match pages.content.get(page_id) {
            PageContent::Edit => assert!(editor_ids.len() == 2),
            PageContent::OpenFile => assert!(editor_ids.len() == 3),
        }
        for editor_id in editor_ids {
            assert!(editor_id.0 < app.editors.editor_count);
        }
        let editor_rects = pages.editor_rects.get(page_id);
        let last_draw_size = *pages.last_draw_size.get(page_id);
        assert!(editor_rects.len() == editor_ids.len());
        for rect in editor_rects {
            assert!(rect.pos[0] + rect.size[0] <= last_draw_size[0]);
            assert!(rect.pos[1] + rect.size[1] <= last_draw_size[1]);
        }
        for (ix0, rect0) in editor_rects.iter().enumerate() {
            for (ix1, rect1) in editor_rects.iter().enumerate() {
                if ix0 != ix1 {
                    let overlap = Rect::intersect(*rect0, *rect1).size;
                    assert!(overlap[0] == 0.0 || overlap[1] == 0.0);
                }
            }
        }
        assert!(*pages.focus.get(page_id) < editor_ids.len());
    }
}

impl PageId {
    pub(crate) fn tick(self, app: &mut App, io: &mut dyn IO) {
        match *app.pages.content.get(self) {
            PageContent::Edit => {
                let editor_ids = app.pages.editor_ids.get(self).clone();
                let [editor_id, status_bar_id] = &*editor_ids else {
                    unreachable!()
                };

                editor_id.tick(app, io);

                // Update status bar text.
                let status_bar_buffer_id = *app.editors.buffer_id.get(*status_bar_id);
                let cursor_main_offset = app
                    .editors
                    .cursors
                    .get(*editor_id)
                    .last()
                    .unwrap()
                    .head
                    .offset;
                let grid = editor_id.grid_from_offset(app, cursor_main_offset);
                let buffer_id = *app.editors.buffer_id.get(*editor_id);
                let source = match buffer_id.source(app) {
                    Source::Scratch => BStr::new("scratch"),
                    Source::File(SourceFile { absolute_path, .. }) => {
                        BStr::new(absolute_path.as_os_str().as_bytes())
                    }
                };
                let status_text = format!("{} {}:{}", source, grid[0][1] + 1, grid[0][0] + 1);
                status_bar_buffer_id.replace(app, BStr::new(status_text.as_bytes()));

                status_bar_id.tick(app, io);
            }
            PageContent::OpenFile => {
                // TODO
            }
        }
    }

    pub(crate) fn input(self, app: &mut App, io: &mut dyn IO, mut event: InputEvent<'_>) {
        // Update drag state.
        if let InputEvent::MouseButton { state, .. } = event {
            *app.pages.dragging.get_mut(self) = state == ButtonState::Pressed;
        }

        // Check if focus changed.
        if let InputEvent::MouseMoved { position } = event {
            if !*app.pages.dragging.get(self) {
                let editor_rects = app.pages.editor_rects.get(self).clone();
                let focus = *app.pages.focus.get(self);
                let focus_new = (0..editor_rects.len())
                    .filter(|i| editor_rects[*i].contains(position))
                    .next()
                    .unwrap_or(focus);
                if focus != focus_new {
                    *app.pages.focus.get_mut(self) = focus_new;
                    let editor_ids = app.pages.editor_ids.get(self).clone();
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

        let focus = *app.pages.focus.get(self);
        let editor_rect = app.pages.editor_rects.get(self)[focus];

        // Adjust mouse event positions to be relative to the focused editor.
        match &mut event {
            InputEvent::MouseMoved { position } | InputEvent::MouseButton { position, .. } => {
                position[0] -= editor_rect.pos[0];
                position[1] -= editor_rect.pos[1];
            }
            _ => {}
        }

        // Dispatch event to focused editor.
        let editor_id = app.pages.editor_ids.get(self)[focus];
        editor_id.input(app, io, event);
    }

    pub(crate) fn draw(self, app: &mut App, drawing: &mut Drawing) {
        let cell_size = app.cell_size();

        // Resize if necessary.
        if *app.pages.last_draw_size.get(self) != drawing.size() {
            *app.pages.last_draw_size.get_mut(self) = drawing.size();
            let page_rect = Rect {
                pos: [0.0, 0.0],
                size: drawing.size(),
            };
            *app.pages.editor_rects.get_mut(self) = match *app.pages.content.get(self) {
                PageContent::Edit => {
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
        }

        // Draw gaps.
        let last_draw_size = *app.pages.last_draw_size.get(self);
        drawing.draw_rect(
            Rect {
                pos: [0.0, 0.0],
                size: last_draw_size,
            },
            BACKGROUND_COLOR,
        );
        drawing.draw_rect(
            Rect {
                pos: [0.0, 0.0],
                size: last_draw_size,
            },
            HIGHLIGHT_COLOR,
        );

        // Draw each editor.
        let editor_ids = app.pages.editor_ids.get(self).clone();
        let editor_rects = app.pages.editor_rects.get(self).clone();
        let focus = *app.pages.focus.get(self);
        for (i, (editor_id, editor_rect)) in editor_ids.iter().zip(editor_rects.iter()).enumerate()
        {
            let mut drawing = drawing.push_clip_rect(*editor_rect);
            editor_id.draw(app, &mut drawing, focus == i);
        }
    }

    pub(crate) fn handle_edits(self, app: &mut App, editor_ix: usize, _diff: &OffsetDiff) {
        match *app.pages.content.get(self) {
            PageContent::Edit => {}
            PageContent::OpenFile => {
                if editor_ix == 1 {
                    // TODO selection changed, reload lister
                }
            }
        }
    }
}
