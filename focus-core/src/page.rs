use std::path::PathBuf;

use crate::{
    app::{App, IO},
    buffer::OffsetDiff,
    drawing::{Drawing, Rect},
    editor::EditorId,
    input::{ButtonState, InputEvent},
    map::Map,
    style::{BACKGROUND_COLOR, HIGHLIGHT_COLOR},
    window::WindowId,
};

mod search_buffer;
mod search_repo;
mod edit;
mod open_buffer;
mod open_file;
mod open_file_from_repo;

pub(crate) use search_buffer::new as new_search_buffer;
pub(crate) use search_repo::new as new_search_repo;
pub(crate) use edit::new as new_edit;
pub(crate) use open_buffer::new as new_open_buffer;
pub(crate) use open_file::new as new_open_file;
pub(crate) use open_file_from_repo::new as new_open_file_from_repo;
pub(crate) use search_buffer::new as new_search_buffer;

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy, Debug)]
pub struct PageId(pub(crate) usize);

pub struct Pages {
    pub(crate) page_count: usize,

    content: Map<PageId, PageContent>,
    pub(crate) editor_ids: Map<PageId, Vec<EditorId>>,
    editor_rects: Map<PageId, Vec<Rect>>,
    focus: Map<PageId, usize>,
    dragging: Map<PageId, bool>,
    last_draw_size: Map<PageId, [f32; 2]>,
}

enum PageContent {
    SearchBuffer(search_buffer::State),
    SearchRepo(search_repo::State),
    Edit,
    OpenBuffer(open_buffer::State),
    OpenFile,
    OpenFileFromRepo(open_file_from_repo::State),
}

#[derive(Clone, Copy)]
enum PageContentKind {
    SearchBuffer,
    SearchRepo,
    Edit,
    OpenBuffer,
    OpenFile,
    OpenFileFromRepo,
}

impl PageContent {
    fn kind(&self) -> PageContentKind {
        match self {
            PageContent::SearchBuffer(_) => PageContentKind::SearchBuffer,
            PageContent::SearchRepo(_) => PageContentKind::SearchRepo,
            PageContent::Edit => PageContentKind::Edit,
            PageContent::OpenBuffer(_) => PageContentKind::OpenBuffer,
            PageContent::OpenFile => PageContentKind::OpenFile,
            PageContent::OpenFileFromRepo(_) => PageContentKind::OpenFileFromRepo,
        }
    }
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
        let editor_ids = &pages.editor_ids[page_id];
        match pages.content[page_id].kind() {
            PageContentKind::SearchBuffer => {
                assert!(editor_ids.len() == search_buffer::EDITOR_COUNT)
            }
            PageContentKind::SearchRepo => {
                assert!(editor_ids.len() == search_repo::EDITOR_COUNT)
            }
            PageContentKind::Edit => assert!(editor_ids.len() == edit::EDITOR_COUNT),
            PageContentKind::OpenBuffer => assert!(editor_ids.len() == open_buffer::EDITOR_COUNT),
            PageContentKind::OpenFile => assert!(editor_ids.len() == open_file::EDITOR_COUNT),
            PageContentKind::OpenFileFromRepo => {
                assert!(editor_ids.len() == open_file_from_repo::EDITOR_COUNT)
            }
        }
        for editor_id in editor_ids {
            assert!(editor_id.0 < app.editors.editor_count);
        }
        let editor_rects = &pages.editor_rects[page_id];
        let last_draw_size = pages.last_draw_size[page_id];
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
        assert!(pages.focus[page_id] < editor_ids.len());
    }
}

impl PageId {
    pub(crate) fn tick(self, app: &mut App, io: &mut dyn IO) {
        match app.pages.content[self].kind() {
            PageContentKind::SearchBuffer => search_buffer::tick(self, app, io),
            PageContentKind::SearchRepo => search_repo::tick(self, app, io),
            PageContentKind::Edit => edit::tick(self, app, io),
            PageContentKind::OpenBuffer => open_buffer::tick(self, app, io),
            PageContentKind::OpenFile => open_file::tick(self, app, io),
            PageContentKind::OpenFileFromRepo => open_file_from_repo::tick(self, app, io),
        }
    }

    pub(crate) fn input(
        self,
        app: &mut App,
        io: &mut dyn IO,
        window_id: WindowId,
        mut event: InputEvent<'_>,
    ) {
        // Update drag state.
        if let InputEvent::MouseButton { state, .. } = event {
            app.pages.dragging[self] = state == ButtonState::Pressed;
        }

        // Check if focus changed.
        if let InputEvent::MouseMoved { position } = event
            && !app.pages.dragging[self]
        {
            let focus = app.pages.focus[self];
            let focus_new = app.pages.editor_rects[self]
                .iter()
                .position(|rect| rect.contains(position))
                .unwrap_or(focus);
            if focus != focus_new {
                app.pages.focus[self] = focus_new;
                let editor_count = app.pages.editor_ids[self].len();
                for i in 0..editor_count {
                    let editor_id = app.pages.editor_ids[self][i];
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

        let handled = match app.pages.content[self].kind() {
            PageContentKind::SearchBuffer => search_buffer::input(self, app, io, window_id, &event),
            PageContentKind::SearchRepo => search_repo::input(self, app, io, window_id, &event),
            PageContentKind::Edit => edit::input(self, app, io, window_id, &event),
            PageContentKind::OpenBuffer => open_buffer::input(self, app, io, window_id, &event),
            PageContentKind::OpenFile => open_file::input(self, app, io, window_id, &event),
            PageContentKind::OpenFileFromRepo => {
                open_file_from_repo::input(self, app, io, window_id, &event)
            }
        };
        if handled {
            return;
        }

        let focus = app.pages.focus[self];
        let editor_rect = app.pages.editor_rects[self][focus];

        // Adjust mouse event positions to be relative to the focused editor.
        match &mut event {
            InputEvent::MouseMoved { position } | InputEvent::MouseButton { position, .. } => {
                position[0] -= editor_rect.pos[0];
                position[1] -= editor_rect.pos[1];
            }
            _ => {}
        }

        // Dispatch event to focused editor.
        let editor_id = app.pages.editor_ids[self][focus];
        editor_id.input(app, io, event);
    }

    pub(crate) fn draw(self, app: &mut App, drawing: &mut Drawing) {
        let cell_size = app.cell_size();

        // Resize if necessary.
        if app.pages.last_draw_size[self] != drawing.size() {
            app.pages.last_draw_size[self] = drawing.size();
            let page_rect = Rect {
                pos: [0.0, 0.0],
                size: drawing.size(),
            };
            app.pages.editor_rects[self] = match app.pages.content[self].kind() {
                PageContentKind::SearchBuffer => search_buffer::layout(page_rect, cell_size),
                PageContentKind::SearchRepo => search_repo::layout(page_rect, cell_size),
                PageContentKind::Edit => edit::layout(page_rect, cell_size),
                PageContentKind::OpenBuffer => open_buffer::layout(page_rect, cell_size),
                PageContentKind::OpenFile => open_file::layout(page_rect, cell_size),
                PageContentKind::OpenFileFromRepo => {
                    open_file_from_repo::layout(page_rect, cell_size)
                }
            };
        }

        // Draw gaps.
        let last_draw_size = app.pages.last_draw_size[self];
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
        let editor_count = app.pages.editor_ids[self].len();
        let focus = app.pages.focus[self];
        for i in 0..editor_count {
            let editor_id = app.pages.editor_ids[self][i];
            let editor_rect = app.pages.editor_rects[self][i];
            let mut drawing = drawing.push_clip_rect(editor_rect);
            editor_id.draw(app, &mut drawing, focus == i);
        }
    }

    pub(crate) fn handle_edits(self, app: &mut App, editor_ix: usize, diff: &OffsetDiff) {
        match app.pages.content[self].kind() {
            PageContentKind::SearchBuffer => {
                search_buffer::handle_edits(self, app, editor_ix, diff)
            }
            PageContentKind::SearchRepo => search_repo::handle_edits(self, app, editor_ix, diff),
            PageContentKind::Edit => edit::handle_edits(self, app, editor_ix, diff),
            PageContentKind::OpenBuffer => open_buffer::handle_edits(self, app, editor_ix, diff),
            PageContentKind::OpenFile => open_file::handle_edits(self, app, editor_ix, diff),
            PageContentKind::OpenFileFromRepo => {
                open_file_from_repo::handle_edits(self, app, editor_ix, diff)
            }
        }
    }

    pub(crate) fn current_path(self, app: &App) -> Option<PathBuf> {
        match app.pages.content[self].kind() {
            PageContentKind::SearchBuffer => search_buffer::current_path(self, app),
            PageContentKind::SearchRepo => search_repo::current_path(self, app),
            PageContentKind::Edit => edit::current_path(self, app),
            PageContentKind::OpenBuffer => open_buffer::current_path(self, app),
            PageContentKind::OpenFile => open_file::current_path(self, app),
            PageContentKind::OpenFileFromRepo => open_file_from_repo::current_path(self, app),
        }
    }
}
