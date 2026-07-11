use std::ops::Range;
use std::path::PathBuf;

use bstr::{BStr, BString, ByteSlice};

use crate::{
    app::{App, IO},
    buffer::{BufferId, OffsetDiff},
    drawing::Rect,
    editor::{self, EditorId},
    input::{ButtonState, InputEvent, Key, NamedKey},
    window::WindowId,
};

use super::{GAP, PageContent, PageId, insert, new_edit};

#[derive(Clone)]
pub(super) struct State {
    buffer_id: BufferId,
    initial_offset: usize,
    needs_initial_selection: bool,
    matches: Vec<Match>,
    list_text: BString,
    last_pattern: Option<BString>,
}

#[derive(Clone)]
struct Match {
    range: Range<usize>,
    display: BString,
}

#[derive(Clone, Copy)]
struct BufferSearchEditors {
    preview_id: EditorId,
    search_id: EditorId,
    list_id: EditorId,
}

pub(super) const EDITOR_COUNT: usize = 3;

const PREVIEW_IX: usize = 0;
const SEARCH_IX: usize = 1;
const LIST_IX: usize = 2;

pub(crate) fn new(app: &mut App, buffer_id: BufferId, initial_offset: usize) -> PageId {
    let preview_id = editor::new(app, buffer_id);
    let search_id = editor::new_scratch(app);
    let list_id = editor::new_scratch(app);

    let search_text = app.buffer_search_text.clone();
    let search_buffer_id = app.editors.buffer_id[search_id];
    search_buffer_id.replace(app, search_text.as_bstr());
    if search_text.is_empty() {
        search_id.cursor_goto_buffer_end(app);
    } else {
        search_id.set_marked_ranges(app, &[0..search_text.len()]);
    }

    insert(
        app,
        PageContent::BufferSearch(State {
            buffer_id,
            initial_offset,
            needs_initial_selection: true,
            matches: Vec::new(),
            list_text: BString::default(),
            last_pattern: None,
        }),
        vec![preview_id, search_id, list_id],
        SEARCH_IX,
    )
}

pub(super) fn tick(page_id: PageId, app: &mut App, io: &mut dyn IO) {
    let BufferSearchEditors {
        preview_id,
        search_id,
        list_id,
    } = editors(app, page_id);

    preview_id.tick(app, io);
    search_id.tick(app, io);

    refresh_matches(app, page_id, search_id);

    let list_buffer_id = app.editors.buffer_id[list_id];
    let list_changed = {
        let list_text = &state(app, page_id).list_text;
        list_buffer_id.text(app) != list_text.as_bstr()
    };
    if list_changed {
        let list_text = state(app, page_id).list_text.clone();
        list_buffer_id.replace(app, list_text.as_bstr());
    }

    let initial_selection = if state(app, page_id).needs_initial_selection {
        initial_match_ix(app, page_id)
    } else {
        None
    };
    if state(app, page_id).needs_initial_selection {
        if let Some(match_ix) = initial_selection {
            select_match_line(app, list_id, match_ix);
        } else {
            list_id.cursor_reset(app);
        }
        state_mut(app, page_id).needs_initial_selection = false;
    } else if list_changed {
        list_id.cursor_reset(app);
    }

    app.editors.gutter_marker[list_id] = has_selection(app, page_id);

    if let Some(range) = selected_match_range(app, page_id, list_id) {
        preview_id.set_marked_ranges(app, &[range.clone()]);
        preview_id.scroll_offset_into_center(app, range.start);
    } else {
        preview_id.cursor_reset(app);
    }

    list_id.tick(app, io);
}

pub(super) fn input(
    page_id: PageId,
    app: &mut App,
    io: &mut dyn IO,
    window_id: WindowId,
    event: &InputEvent<'_>,
) -> bool {
    // Enter chords work whichever editor is focused. Plain enter goes to the
    // focused editor like any other key.
    if let InputEvent::Key {
        state: ButtonState::Pressed,
        logical_key: Key::Named(NamedKey::Enter),
    } = event
    {
        match (app.modifiers.control, app.modifiers.alt) {
            (true, false) => {
                submit_selected(page_id, app, io, window_id);
                return true;
            }
            (false, true) => {
                submit_all(page_id, app, io, window_id);
                return true;
            }
            _ => {}
        }
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

pub(super) fn handle_edits(page_id: PageId, app: &mut App, editor_ix: usize, _diff: &OffsetDiff) {
    match editor_ix {
        PREVIEW_IX => state_mut(app, page_id).last_pattern = None,
        SEARCH_IX => {
            let search_id = editors(app, page_id).search_id;
            let search_buffer_id = app.editors.buffer_id[search_id];
            app.buffer_search_text = search_buffer_id.text(app).into();
            state_mut(app, page_id).last_pattern = None;
        }
        LIST_IX => {}
        _ => unreachable!(),
    }
}

pub(super) fn current_path(_page_id: PageId, _app: &App) -> Option<PathBuf> {
    None
}

fn editors(app: &App, page_id: PageId) -> BufferSearchEditors {
    let &[preview_id, search_id, list_id] = app.pages.editor_ids[page_id].as_slice() else {
        unreachable!()
    };
    BufferSearchEditors {
        preview_id,
        search_id,
        list_id,
    }
}

fn state(app: &App, page_id: PageId) -> &State {
    let PageContent::BufferSearch(state) = &app.pages.content[page_id] else {
        unreachable!()
    };
    state
}

fn state_mut(app: &mut App, page_id: PageId) -> &mut State {
    let PageContent::BufferSearch(state) = &mut app.pages.content[page_id] else {
        unreachable!()
    };
    state
}

fn refresh_matches(app: &mut App, page_id: PageId, search_id: EditorId) {
    let search_buffer_id = app.editors.buffer_id[search_id];
    let pattern = BString::from(search_buffer_id.text(app).to_vec());
    app.buffer_search_text = pattern.clone();

    let state = state(app, page_id);
    if state.last_pattern.as_ref() == Some(&pattern) {
        return;
    }

    let buffer_id = state.buffer_id;
    let matches = matches_for_buffer(app, buffer_id, pattern.as_bstr());
    let list_text = matches_text(&matches);

    let state = state_mut(app, page_id);
    state.last_pattern = Some(pattern);
    state.matches = matches;
    state.list_text = list_text;
}

fn matches_for_buffer(app: &App, buffer_id: BufferId, pattern: &BStr) -> Vec<Match> {
    if pattern.is_empty() {
        return Vec::new();
    }

    let mut matches = Vec::new();
    let text = buffer_id.text(app);
    let mut search_start = 0;
    while let Some(relative_start) = text[search_start..].find(pattern) {
        let start = search_start + relative_start;
        let end = start + pattern.len();
        let line = buffer_id.grid_from_offset(app, start)[1];
        let line_range = buffer_id.line_range_from_offset(app, start);
        let mut display = BString::from(format!("{} ", line + 1));
        display.extend_from_slice(&text[line_range]);
        matches.push(Match {
            range: start..end,
            display,
        });
        search_start = end;
    }
    matches
}

fn matches_text(matches: &[Match]) -> BString {
    let lines = matches.iter().map(|entry| entry.display.clone());
    bstr::join("\n", lines).into()
}

fn initial_match_ix(app: &App, page_id: PageId) -> Option<usize> {
    let state = state(app, page_id);
    if state.matches.is_empty() {
        return None;
    }
    state
        .matches
        .iter()
        .position(|entry| entry.range.start > state.initial_offset)
        .or(Some(0))
}

fn select_match_line(app: &mut App, list_id: EditorId, match_ix: usize) {
    let list_buffer_id = app.editors.buffer_id[list_id];
    let offset = line_start_offset(list_buffer_id.text(app), match_ix);
    list_id.set_cursor_offsets(app, &[offset]);
}

fn line_start_offset(text: &BStr, line_ix: usize) -> usize {
    if line_ix == 0 {
        return 0;
    }
    let mut line = 0;
    for (offset, &byte) in text.iter().enumerate() {
        if byte == b'\n' {
            line += 1;
            if line == line_ix {
                return offset + 1;
            }
        }
    }
    text.len()
}

fn has_selection(app: &App, page_id: PageId) -> bool {
    !state(app, page_id).matches.is_empty()
}

fn selected_match_range(app: &App, page_id: PageId, list_id: EditorId) -> Option<Range<usize>> {
    let line = selected_line(app, list_id);
    Some(state(app, page_id).matches.get(line)?.range.clone())
}

fn selected_lines(app: &App, list_id: EditorId) -> Vec<usize> {
    let list_buffer_id = app.editors.buffer_id[list_id];
    app.editors.cursors[list_id]
        .iter()
        .map(|cursor| list_buffer_id.grid_from_offset(app, cursor.head.offset)[1])
        .collect()
}

fn selected_line(app: &App, list_id: EditorId) -> usize {
    selected_lines(app, list_id).last().copied().unwrap_or(0)
}

fn submit_selected(page_id: PageId, app: &mut App, _io: &mut dyn IO, window_id: WindowId) {
    let BufferSearchEditors {
        search_id, list_id, ..
    } = editors(app, page_id);
    refresh_matches(app, page_id, search_id);

    let state = state(app, page_id);
    let ranges: Vec<Range<usize>> = if state.needs_initial_selection {
        initial_match_ix(app, page_id)
            .and_then(|line| state.matches.get(line))
            .map(|entry| vec![entry.range.clone()])
            .unwrap_or_default()
    } else {
        selected_lines(app, list_id)
            .into_iter()
            .filter_map(|line| state.matches.get(line).map(|entry| entry.range.clone()))
            .collect()
    };
    open_edit_with_ranges(app, window_id, state.buffer_id, &ranges);
}

fn submit_all(page_id: PageId, app: &mut App, _io: &mut dyn IO, window_id: WindowId) {
    let search_id = editors(app, page_id).search_id;
    refresh_matches(app, page_id, search_id);

    let state = state(app, page_id);
    let ranges: Vec<Range<usize>> = state
        .matches
        .iter()
        .map(|entry| entry.range.clone())
        .collect();
    open_edit_with_ranges(app, window_id, state.buffer_id, &ranges);
}

fn open_edit_with_ranges(
    app: &mut App,
    window_id: WindowId,
    buffer_id: BufferId,
    ranges: &[Range<usize>],
) {
    if ranges.is_empty() {
        return;
    }
    let editor_id = editor::new(app, buffer_id);
    editor_id.set_marked_ranges(app, ranges);
    editor_id.scroll_offset_into_center(app, ranges.last().unwrap().end);
    app.windows.page_id[window_id] = new_edit(app, editor_id);
}
