use std::{os::unix::ffi::OsStrExt, path::PathBuf};

use bstr::BStr;

use crate::{
    app::{App, IO},
    buffer::{OffsetDiff, Source, SourceFile},
    drawing::Rect,
    editor::{self, EditorId},
    input::{ButtonState, InputEvent, Key},
    window::WindowId,
};

use super::{GAP, PageContent, PageId, insert};

pub(super) const EDITOR_COUNT: usize = 2;

const EDITOR_IX: usize = 0;

#[derive(Clone, Copy)]
struct EditEditors {
    editor_id: EditorId,
    status_bar_id: EditorId,
}

pub(crate) fn new(app: &mut App, editor_id: EditorId) -> PageId {
    let status_bar_id = editor::new_scratch(app);
    insert(
        app,
        PageContent::Edit,
        vec![editor_id, status_bar_id],
        EDITOR_IX,
    )
}

pub(super) fn tick(page_id: PageId, app: &mut App, io: &mut dyn IO) {
    let EditEditors {
        editor_id,
        status_bar_id,
    } = editors(app, page_id);

    editor_id.tick(app, io);

    // Update status bar text.
    let status_bar_buffer_id = app.editors.buffer_id[status_bar_id];
    let cursor_main_offset = app.editors.cursors[editor_id].last().unwrap().head.offset;
    let grid = editor_id.grid_from_offset(app, cursor_main_offset);
    let buffer_id = app.editors.buffer_id[editor_id];
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

pub(super) fn input(
    page_id: PageId,
    app: &mut App,
    io: &mut dyn IO,
    window_id: WindowId,
    event: &InputEvent<'_>,
) -> bool {
    if let InputEvent::Key {
        state: ButtonState::Pressed,
        logical_key: Key::Character("f"),
    } = event
        && app.modifiers.control
        && !app.modifiers.alt
        && app.pages.focus[page_id] == EDITOR_IX
    {
        let editor_id = editors(app, page_id).editor_id;
        let buffer_id = app.editors.buffer_id[editor_id];
        let initial_offset = editor_id.main_cursor_offset(app);
        let page_id = super::new_search_buffer(app, buffer_id, initial_offset);
        window_id.push_page(app, io, page_id);
        return true;
    }

    false
}

pub(super) fn layout(page_rect: Rect, cell_size: [u32; 2]) -> Vec<Rect> {
    let [editor_rect, status_bar_rect] = page_rect.split_from_bottom(cell_size[1] as f32, GAP);
    vec![editor_rect, status_bar_rect]
}

pub(super) fn handle_edits(
    _page_id: PageId,
    _app: &mut App,
    _editor_ix: usize,
    _diff: &OffsetDiff,
) {
}

pub(super) fn current_path(page_id: PageId, app: &App) -> Option<PathBuf> {
    let editor_id = editors(app, page_id).editor_id;
    let buffer_id = app.editors.buffer_id[editor_id];
    buffer_id.path(app)
}

fn editors(app: &App, page_id: PageId) -> EditEditors {
    let &[editor_id, status_bar_id] = app.pages.editor_ids[page_id].as_slice() else {
        unreachable!()
    };
    EditEditors {
        editor_id,
        status_bar_id,
    }
}
