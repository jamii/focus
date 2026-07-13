use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use bstr::{BStr, BString, ByteSlice};

use crate::{
    app::{App, IO},
    buffer::{BufferId, OffsetDiff},
    drawing::Rect,
    editor::{self, EditorId},
    fuzzy,
    input::{ButtonState, InputEvent, Key, NamedKey},
    window::WindowId,
};

use super::{GAP, PageContent, PageId, insert, new_edit};

#[derive(Clone)]
pub(super) struct State {
    empty_preview_buffer_id: BufferId,
    buffers: Vec<OpenBufferEntry>,
    matches: Vec<OpenBufferEntry>,
    list_text: BString,
    last_pattern: Option<BString>,
}

#[derive(Clone, PartialEq, Eq)]
struct OpenBufferEntry {
    buffer_id: BufferId,
    display: BString,
}

#[derive(Clone, Copy)]
struct OpenBufferEditors {
    preview_id: EditorId,
    search_id: EditorId,
    list_id: EditorId,
}

pub(super) const EDITOR_COUNT: usize = 3;

const SEARCH_IX: usize = 1;

pub(crate) fn new(app: &mut App) -> PageId {
    let preview_id = editor::new_scratch(app);
    let search_id = editor::new_scratch(app);
    let list_id = editor::new_scratch(app);
    let empty_preview_buffer_id = app.editors.buffer_id[preview_id];

    insert(
        app,
        PageContent::OpenBuffer(State {
            empty_preview_buffer_id,
            buffers: Vec::new(),
            matches: Vec::new(),
            list_text: BString::default(),
            last_pattern: None,
        }),
        vec![preview_id, search_id, list_id],
        SEARCH_IX,
    )
}

pub(super) fn tick(page_id: PageId, app: &mut App, io: &mut dyn IO) {
    let OpenBufferEditors {
        preview_id,
        search_id,
        list_id,
    } = editors(app, page_id);

    search_id.tick(app, io);

    // Update list text: matching file-backed buffers.
    refresh_matches(app, page_id, search_id);
    let list_buffer_id = app.editors.buffer_id[list_id];
    let list_changed = {
        let list_text = &state(app, page_id).list_text;
        list_buffer_id.text(app) != list_text.as_bstr()
    };
    if list_changed {
        let list_text = state(app, page_id).list_text.clone();
        list_buffer_id.replace(app, list_text.as_bstr());
        // The list changed, so select the closest match again.
        list_id.cursor_reset(app);
    }

    // Mark the selected line, if there is one.
    app.editors.gutter_marker[list_id] = has_selection(app, page_id);

    // Preview the selected buffer directly. If nothing is selected, preview the
    // empty scratch buffer created with the page.
    let preview_buffer_id = selected_buffer_id(app, page_id, list_id)
        .unwrap_or_else(|| state(app, page_id).empty_preview_buffer_id);
    preview_id.set_buffer(app, preview_buffer_id);

    preview_id.tick(app, io);
    list_id.tick(app, io);
}

pub(super) fn input(
    page_id: PageId,
    app: &mut App,
    io: &mut dyn IO,
    window_id: WindowId,
    event: &InputEvent<'_>,
) -> bool {
    // Ctrl+enter opens the selected buffer. Plain enter goes to the focused
    // editor like any other key; alt+enter is intentionally ignored by the
    // page, and the editor has no binding for it.
    if let InputEvent::Key {
        state: ButtonState::Pressed,
        logical_key: Key::Named(NamedKey::Enter),
    } = event
        && app.modifiers.control
        && !app.modifiers.alt
    {
        submit(page_id, app, io, window_id);
        return true;
    }

    // Ctrl+i/k always move the list cursor, so the selection can be changed
    // while typing in the search editor.
    if let InputEvent::Key {
        state: ButtonState::Pressed,
        logical_key: Key::Character("i" | "k"),
    } = event
        && app.modifiers.control
        && !app.modifiers.alt
    {
        let list_id = editors(app, page_id).list_id;
        list_id.input(app, io, (*event).clone());
        return true;
    }

    false
}

pub(super) fn layout(page_rect: Rect, cell_size: [u32; 2]) -> Vec<Rect> {
    let [preview_rect, rest] = page_rect.split_from_bottom(page_rect.size[1] / 2.0, GAP);
    let [search_rect, list_rect] = rest.split_from_top(cell_size[1] as f32, GAP);
    vec![preview_rect, search_rect, list_rect]
}

pub(super) fn handle_edits(
    _page_id: PageId,
    _app: &mut App,
    _editor_ix: usize,
    _diff: &OffsetDiff,
) {
    // The list and preview are refreshed during tick.
}

pub(super) fn current_path(_page_id: PageId, _app: &App) -> Option<PathBuf> {
    None
}

fn editors(app: &App, page_id: PageId) -> OpenBufferEditors {
    let &[preview_id, search_id, list_id] = app.pages.editor_ids[page_id].as_slice() else {
        unreachable!()
    };
    OpenBufferEditors {
        preview_id,
        search_id,
        list_id,
    }
}

fn state(app: &App, page_id: PageId) -> &State {
    let PageContent::OpenBuffer(state) = &app.pages.content[page_id] else {
        unreachable!()
    };
    state
}

fn state_mut(app: &mut App, page_id: PageId) -> &mut State {
    let PageContent::OpenBuffer(state) = &mut app.pages.content[page_id] else {
        unreachable!()
    };
    state
}

// Replace this picker with the selected buffer.
fn submit(page_id: PageId, app: &mut App, io: &mut dyn IO, window_id: WindowId) {
    let OpenBufferEditors {
        search_id, list_id, ..
    } = editors(app, page_id);
    refresh_matches(app, page_id, search_id);
    let Some(buffer_id) = selected_buffer_id(app, page_id, list_id) else {
        return;
    };
    let editor_id = editor::new(app, buffer_id);
    let page_id = new_edit(app, editor_id);
    window_id.replace_page(app, io, page_id);
}

fn refresh_matches(app: &mut App, page_id: PageId, search_id: EditorId) {
    let search_buffer_id = app.editors.buffer_id[search_id];
    let pattern = BString::from(search_buffer_id.text(app).to_vec());
    let buffers = file_buffers(app);
    let state = state_mut(app, page_id);
    if state.last_pattern.as_ref() == Some(&pattern) && state.buffers == buffers {
        return;
    }
    state.last_pattern = Some(pattern.clone());
    state.buffers = buffers;
    state.matches.clear();
    let mut scored: Vec<(fuzzy::Score, OpenBufferEntry)> = state
        .buffers
        .iter()
        .filter_map(|entry| {
            Some((
                fuzzy::score(&entry.display, pattern.as_bstr())?,
                entry.clone(),
            ))
        })
        .collect();
    scored.sort_by(|(score_a, entry_a), (score_b, entry_b)| {
        score_a
            .cmp(score_b)
            .then_with(|| entry_a.display.cmp(&entry_b.display))
    });
    state.matches = scored.into_iter().map(|(_, entry)| entry).collect();
    state.list_text = matches_text(&state.matches);
}

fn file_buffers(app: &App) -> Vec<OpenBufferEntry> {
    let mut entries = Vec::new();
    for buffer_id in app.buffers.keys() {
        let Some(path) = buffer_id.path(app) else {
            continue;
        };
        entries.push(OpenBufferEntry {
            buffer_id,
            display: path_bytes(&path),
        });
    }
    entries.sort_by(|a, b| a.display.cmp(&b.display));
    entries
}

fn matches_text(matches: &[OpenBufferEntry]) -> BString {
    let lines = matches.iter().map(|entry| entry.display.clone());
    bstr::join("\n", lines).into()
}

fn has_selection(app: &App, page_id: PageId) -> bool {
    !state(app, page_id).matches.is_empty()
}

fn selected_buffer_id(app: &App, page_id: PageId, list_id: EditorId) -> Option<BufferId> {
    Some(selected_entry(app, list_id, &state(app, page_id).matches)?.buffer_id)
}

// The entry on the same line as the list editor's main cursor.
fn selected_entry<'a>(
    app: &App,
    list_id: EditorId,
    entries: &'a [OpenBufferEntry],
) -> Option<&'a OpenBufferEntry> {
    let list_buffer_id = app.editors.buffer_id[list_id];
    let offset = app.editors.cursors[list_id].last().unwrap().head.offset;
    let line = list_buffer_id.grid_from_offset(app, offset)[1];
    entries.get(line)
}

fn path_bytes(path: &Path) -> BString {
    BStr::new(path.as_os_str().as_bytes()).into()
}
