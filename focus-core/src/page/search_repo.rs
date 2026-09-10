use std::ops::Range;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use bstr::{BString, ByteSlice};

use crate::{
    app::{App, IO, RepoMatch, RepoSearch},
    buffer::{self, OffsetDiff},
    drawing::Rect,
    editor::{self, EditorId},
    input::{ButtonState, InputEvent, Key, NamedKey},
    window::{self, WindowId},
};

use super::{GAP, PageContent, PageId, insert, new_edit};

#[derive(Clone)]
pub(super) struct State {
    dir: PathBuf,
    root: PathBuf,
    matches: Vec<RepoMatch>,
    list_text: BString,
    last_pattern: Option<BString>,
}

#[derive(Clone, Copy)]
struct SearchRepoEditors {
    preview_id: EditorId,
    search_id: EditorId,
    list_id: EditorId,
}

pub(super) const EDITOR_COUNT: usize = 3;

const SEARCH_IX: usize = 1;

// The most matches to ask `repo_search` for. A short pattern in a big
// directory matches most lines of most files, and collecting all of them
// costs more memory than the machine has.
const MATCH_LIMIT: usize = 1000;

// The most bytes of each match's line to ask `repo_search` for. One minified
// file can otherwise hold megabytes on a single line, once per match on that
// line.
const LINE_LIMIT: usize = 500;

pub(crate) fn new(app: &mut App, dir: PathBuf) -> PageId {
    let preview_id = editor::new_generated(app);
    let search_id = editor::new_scratch(app);
    let list_id = editor::new_generated(app);

    let search_text = app.search_buffer_text.clone();
    let search_buffer_id = app.editors.buffer_id[search_id];
    search_buffer_id.replace(app, search_text.as_bstr());
    if search_text.is_empty() {
        search_id.cursor_goto_buffer_end(app);
    } else {
        search_id.set_marked_ranges(app, &[0..search_text.len()]);
    }

    insert(
        app,
        PageContent::SearchRepo(State {
            root: dir.clone(),
            dir,
            matches: Vec::new(),
            list_text: BString::default(),
            last_pattern: None,
        }),
        vec![preview_id, search_id, list_id],
        SEARCH_IX,
    )
}

pub(super) fn duplicate(page_id: PageId, app: &mut App, _io: &mut dyn IO) -> PageId {
    let SearchRepoEditors {
        preview_id,
        search_id,
        list_id,
    } = editors(app, page_id);
    // Clone the captured matches rather than re-running the search.
    let state = state(app, page_id).clone();
    let preview_id = editor::new_copy(app, preview_id);
    let search_id = editor::new_copy(app, search_id);
    let list_id = editor::new_copy(app, list_id);
    let focus = app.pages.focus[page_id];
    insert(
        app,
        PageContent::SearchRepo(state),
        vec![preview_id, search_id, list_id],
        focus,
    )
}

pub(super) fn tick(page_id: PageId, app: &mut App, io: &mut dyn IO) {
    let SearchRepoEditors {
        preview_id,
        search_id,
        list_id,
    } = editors(app, page_id);

    search_id.tick(app, io);

    // Update list text: repo matches for the current pattern, or the search
    // error if there was one.
    refresh_matches(app, io, page_id, search_id);
    let list_buffer_id = app.editors.buffer_id[list_id];
    let list_changed = {
        let list_text = &state(app, page_id).list_text;
        list_buffer_id.text(app) != list_text.as_bstr()
    };
    if list_changed {
        let list_text = state(app, page_id).list_text.clone();
        list_buffer_id.replace(app, list_text.as_bstr());
        // The list changed, so select the first match again.
        list_id.cursor_reset(app);
    }

    // Mark the selected line, if there is one.
    app.editors.gutter_marker[list_id] = has_selection(app, page_id);

    // Update preview text: the selected match's file, with the match marked.
    let selected = selected_match(app, page_id, list_id);
    let preview_text = match &selected {
        Some(entry) => {
            let path = state(app, page_id).root.join(&entry.relative_path);
            match io.file_read(&path) {
                Ok(contents) => BString::from(contents),
                Err(error) => BString::from(error.to_string()),
            }
        }
        None => BString::default(),
    };
    let preview_buffer_id = app.editors.buffer_id[preview_id];
    if preview_buffer_id.text(app) != preview_text.as_bstr() {
        preview_buffer_id.replace(app, preview_text.as_bstr());
        preview_id.cursor_reset(app);
    }
    // The range check fails if the file changed since the search.
    if let Some(entry) = &selected
        && entry.range.end <= preview_text.len()
    {
        preview_id.set_marked_ranges(app, &[entry.range.clone()]);
        preview_id.scroll_offset_into_center(app, entry.range.start);
    }

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

fn editors(app: &App, page_id: PageId) -> SearchRepoEditors {
    let &[preview_id, search_id, list_id] = app.pages.editor_ids[page_id].as_slice() else {
        unreachable!()
    };
    SearchRepoEditors {
        preview_id,
        search_id,
        list_id,
    }
}

fn state(app: &App, page_id: PageId) -> &State {
    let PageContent::SearchRepo(state) = &app.pages.content[page_id] else {
        unreachable!()
    };
    state
}

fn state_mut(app: &mut App, page_id: PageId) -> &mut State {
    let PageContent::SearchRepo(state) = &mut app.pages.content[page_id] else {
        unreachable!()
    };
    state
}

fn refresh_matches(app: &mut App, io: &mut dyn IO, page_id: PageId, search_id: EditorId) {
    let search_buffer_id = app.editors.buffer_id[search_id];
    let pattern = BString::from(search_buffer_id.text(app).to_vec());
    app.search_buffer_text = pattern.clone();

    let state = state(app, page_id);
    if state.last_pattern.as_ref() == Some(&pattern) {
        return;
    }

    let dir = state.dir.clone();
    let (root, matches, list_text) = if pattern.is_empty() {
        (None, Vec::new(), BString::default())
    } else {
        match io.repo_search(&dir, pattern.as_bstr(), MATCH_LIMIT, LINE_LIMIT) {
            Ok(RepoSearch {
                root,
                matches,
                truncated,
            }) => {
                let list_text = matches_text(&matches, truncated);
                (Some(root), matches, list_text)
            }
            Err(error) => (
                None,
                Vec::new(),
                BString::from(format!("{}: {}", dir.display(), error)),
            ),
        }
    };

    let state = state_mut(app, page_id);
    state.last_pattern = Some(pattern);
    state.matches = matches;
    state.list_text = list_text;
    if let Some(root) = root {
        state.root = root;
    }
}

fn matches_text(matches: &[RepoMatch], truncated: bool) -> BString {
    let mut lines: Vec<BString> = matches.iter().map(display_match).collect();
    // The extra line has no match behind it, so selecting it shows no
    // preview and opens nothing.
    if truncated {
        lines.push(BString::from(format!(
            "[first {} matches only]",
            MATCH_LIMIT
        )));
    }
    bstr::join("\n", lines).into()
}

fn display_match(entry: &RepoMatch) -> BString {
    let mut display = BString::from(entry.relative_path.as_os_str().as_bytes());
    display.extend_from_slice(format!(":{} ", entry.line + 1).as_bytes());
    display.extend_from_slice(&entry.line_text);
    display
}

fn has_selection(app: &App, page_id: PageId) -> bool {
    !state(app, page_id).matches.is_empty()
}

fn selected_match(app: &App, page_id: PageId, list_id: EditorId) -> Option<RepoMatch> {
    let line = selected_lines(app, list_id).last().copied().unwrap_or(0);
    state(app, page_id).matches.get(line).cloned()
}

fn selected_lines(app: &App, list_id: EditorId) -> Vec<usize> {
    let list_buffer_id = app.editors.buffer_id[list_id];
    app.editors.cursors[list_id]
        .iter()
        .map(|cursor| list_buffer_id.grid_from_offset(app, cursor.head.offset)[1])
        .collect()
}

fn submit_selected(page_id: PageId, app: &mut App, io: &mut dyn IO, window_id: WindowId) {
    let SearchRepoEditors {
        search_id, list_id, ..
    } = editors(app, page_id);
    refresh_matches(app, io, page_id, search_id);

    let state = state(app, page_id);
    let selected: Vec<RepoMatch> = selected_lines(app, list_id)
        .into_iter()
        .filter_map(|line| state.matches.get(line).cloned())
        .collect();
    // The last cursor is the main one.
    let Some(main) = selected.last() else {
        return;
    };
    let root = state.root.clone();
    let main_path = main.relative_path.clone();
    open_matches(app, io, window_id, &root, &selected, &main_path);
}

fn submit_all(page_id: PageId, app: &mut App, io: &mut dyn IO, window_id: WindowId) {
    let SearchRepoEditors {
        search_id, list_id, ..
    } = editors(app, page_id);
    refresh_matches(app, io, page_id, search_id);

    let state = state(app, page_id);
    if state.matches.is_empty() {
        return;
    }
    let matches = state.matches.clone();
    let root = state.root.clone();
    let main = selected_match(app, page_id, list_id).unwrap_or_else(|| matches[0].clone());
    open_matches(app, io, window_id, &root, &matches, &main.relative_path);
}

// Open one edit page per file: the main match's file replaces this window's
// page, and every other file opens in a new window.
fn open_matches(
    app: &mut App,
    io: &mut dyn IO,
    window_id: WindowId,
    root: &Path,
    matches: &[RepoMatch],
    main_path: &Path,
) {
    let mut groups: Vec<(&Path, Vec<Range<usize>>)> = Vec::new();
    for entry in matches {
        let relative_path = entry.relative_path.as_path();
        match groups.iter_mut().find(|(path, _)| *path == relative_path) {
            Some((_, ranges)) => ranges.push(entry.range.clone()),
            None => groups.push((relative_path, vec![entry.range.clone()])),
        }
    }
    for (relative_path, ranges) in groups {
        let page_id = edit_page_for_matches(app, io, root.join(relative_path), &ranges);
        if relative_path == main_path {
            window_id.replace_page(app, io, page_id);
        } else {
            window::open(app, io, page_id);
        }
    }
}

fn edit_page_for_matches(
    app: &mut App,
    io: &mut dyn IO,
    path: PathBuf,
    ranges: &[Range<usize>],
) -> PageId {
    let buffer_id = buffer::from_file(app, io, path);
    // Load the file now so the match ranges can be marked.
    buffer_id.tick(app, io);
    let editor_id = editor::new(app, buffer_id);
    // Ranges past the end happen if the file changed since the search.
    let len = buffer_id.text(app).len();
    let ranges: Vec<Range<usize>> = ranges
        .iter()
        .filter(|range| range.end <= len)
        .cloned()
        .collect();
    if !ranges.is_empty() {
        editor_id.set_marked_ranges(app, &ranges);
        editor_id.scroll_offset_into_center(app, ranges.last().unwrap().end);
    }
    new_edit(app, editor_id)
}
