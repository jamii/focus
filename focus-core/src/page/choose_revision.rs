// A picker over the repo's revisions, opened with alt+2. Choosing one
// opens the diff page for it, which is the same page ctrl+2 opens for the
// working copy.

use std::path::PathBuf;

use bstr::{BString, ByteSlice};

use crate::{
    app::{App, ID_PREFIX_LEN, IO, VcsRevision, VcsRevisionId},
    buffer::OffsetDiff,
    drawing::Rect,
    editor::{self, EditorId},
    fuzzy,
    input::{ButtonState, InputEvent, Key, NamedKey},
    window::WindowId,
};

use super::{GAP, PageContent, PageId, insert, new_diff};

#[derive(Clone)]
pub(super) struct State {
    root: PathBuf,
    // Every revision the repo reported, and the ones the search text
    // matches. The matches keep the order they were listed in - newest
    // first - rather than being sorted by how well they match: a revision
    // list is read by position as much as by name.
    revisions: Vec<VcsRevision>,
    matches: Vec<VcsRevision>,
    list_text: BString,
    last_pattern: Option<BString>,
    // Shown in place of the list when the revisions cannot be read.
    error: Option<BString>,
}

#[derive(Clone, Copy)]
struct ChooseRevisionEditors {
    search_id: EditorId,
    list_id: EditorId,
}

pub(super) const EDITOR_COUNT: usize = 2;

const SEARCH_IX: usize = 0;

pub(crate) fn new(app: &mut App, root: PathBuf) -> PageId {
    let search_id = editor::new_scratch(app);
    let list_id = editor::new_generated(app);
    insert(
        app,
        PageContent::ChooseRevision(State {
            root,
            revisions: Vec::new(),
            matches: Vec::new(),
            list_text: BString::default(),
            last_pattern: None,
            error: None,
        }),
        vec![search_id, list_id],
        SEARCH_IX,
    )
}

pub(super) fn duplicate(page_id: PageId, app: &mut App, _io: &mut dyn IO) -> PageId {
    let ChooseRevisionEditors { search_id, list_id } = editors(app, page_id);
    let state = state(app, page_id).clone();
    let search_id = editor::new_copy(app, search_id);
    let list_id = editor::new_copy(app, list_id);
    let focus = app.pages.focus[page_id];
    insert(
        app,
        PageContent::ChooseRevision(state),
        vec![search_id, list_id],
        focus,
    )
}

pub(super) fn tick(page_id: PageId, app: &mut App, io: &mut dyn IO, _window_id: WindowId) {
    let ChooseRevisionEditors { search_id, list_id } = editors(app, page_id);

    search_id.tick(app, io);
    refresh_revisions(page_id, app, io);
    refresh_list(page_id, app);

    app.editors.gutter_marker[list_id] = !state(app, page_id).matches.is_empty();
    list_id.tick(app, io);
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
        logical_key: Key::Named(NamedKey::Enter),
    } = event
        && app.modifiers.control
        && !app.modifiers.alt
    {
        submit(page_id, app, io, window_id);
        return true;
    }

    // Ctrl+i/k move the selection while the search field is focused.
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
    let [search_rect, list_rect] = page_rect.split_from_top(cell_size[1] as f32, GAP);
    vec![search_rect, list_rect]
}

pub(super) fn handle_edits(
    _page_id: PageId,
    _app: &mut App,
    _editor_ix: usize,
    _diff: &OffsetDiff,
) {
    // The list is refreshed during tick.
}

pub(super) fn current_path(_page_id: PageId, _app: &App) -> Option<PathBuf> {
    None
}

fn editors(app: &App, page_id: PageId) -> ChooseRevisionEditors {
    let &[search_id, list_id] = app.pages.editor_ids[page_id].as_slice() else {
        unreachable!()
    };
    ChooseRevisionEditors { search_id, list_id }
}

fn state(app: &App, page_id: PageId) -> &State {
    let PageContent::ChooseRevision(state) = &app.pages.content[page_id] else {
        unreachable!()
    };
    state
}

fn state_mut(app: &mut App, page_id: PageId) -> &mut State {
    let PageContent::ChooseRevision(state) = &mut app.pages.content[page_id] else {
        unreachable!()
    };
    state
}

fn submit(page_id: PageId, app: &mut App, io: &mut dyn IO, window_id: WindowId) {
    // Refresh the list first: the cursor line is a position in the list
    // buffer, so a stale list would open whatever the new one happens to
    // hold at that line.
    refresh_list(page_id, app);
    let list_id = editors(app, page_id).list_id;
    let Some(revision) = selected_revision(app, page_id, list_id) else {
        return;
    };
    let root = state(app, page_id).root.clone();
    // The working copy is asked for as the working copy, not by id: it
    // is the one revision whose files are read off disk.
    let revision = if revision.is_working_copy {
        VcsRevisionId::WorkingCopy
    } else {
        VcsRevisionId::Change(revision.change_id)
    };
    let page_id = new_diff(app, root, revision, None);
    window_id.replace_page(app, io, page_id);
}

// Re-read the revisions. They change whenever anything commits, rewrites
// or abandons one, which the editor does not know about, so this is read
// every frame like the diff itself.
fn refresh_revisions(page_id: PageId, app: &mut App, io: &mut dyn IO) {
    let root = state(app, page_id).root.clone();
    let (revisions, error) = match io.vcs_revisions(&root) {
        Ok(revisions) => (revisions, None),
        Err(error) => (Vec::new(), Some(BString::from(error.to_string()))),
    };

    let state = state_mut(app, page_id);
    if state.revisions == revisions && state.error == error {
        return;
    }
    state.revisions = revisions;
    state.error = error;
    // The revisions changed, so the matches are stale even though the
    // search text has not changed.
    state.last_pattern = None;
}

// Bring `matches` and the list buffer in line with the current search
// text. Both change together, so the list buffer's line numbers are
// always positions in `matches`.
fn refresh_list(page_id: PageId, app: &mut App) {
    let ChooseRevisionEditors { search_id, list_id } = editors(app, page_id);
    refresh_matches(app, page_id, search_id);

    let list_buffer_id = app.editors.buffer_id[list_id];
    let list_changed = {
        let list_text = &state(app, page_id).list_text;
        list_buffer_id.text(app) != list_text.as_bstr()
    };
    if list_changed {
        let list_text = state(app, page_id).list_text.clone();
        list_buffer_id.replace(app, list_text.as_bstr());
        list_id.cursor_reset(app);
    }
}

fn refresh_matches(app: &mut App, page_id: PageId, search_id: EditorId) {
    let search_buffer_id = app.editors.buffer_id[search_id];
    let pattern = BString::from(search_buffer_id.text(app).to_vec());
    let state = state_mut(app, page_id);
    if state.last_pattern.as_ref() == Some(&pattern) {
        return;
    }
    state.last_pattern = Some(pattern.clone());
    state.matches = state
        .revisions
        .iter()
        .filter(|revision| fuzzy::score(&display(revision), &pattern).is_some())
        .cloned()
        .collect();
    state.list_text = match &state.error {
        // The list has nothing to show, so the reason is what it shows.
        Some(error) => error.clone(),
        None => bstr::join("\n", state.matches.iter().map(display)).into(),
    };
}

// One revision as a list line: the change id, a mark for the revision the
// working copy is on, and the description.
fn display(revision: &VcsRevision) -> BString {
    let mark = if revision.is_working_copy { "@" } else { " " };
    let id = &revision.change_id[..ID_PREFIX_LEN.min(revision.change_id.len())];
    let description = if revision.description.is_empty() {
        BString::from("(no description set)")
    } else {
        revision.description.clone()
    };
    let mut line = BString::from(format!("{mark} "));
    line.extend_from_slice(id);
    line.push(b' ');
    line.extend_from_slice(&description);
    line
}

fn selected_revision(app: &App, page_id: PageId, list_id: EditorId) -> Option<VcsRevision> {
    let list_buffer_id = app.editors.buffer_id[list_id];
    let offset = list_id.main_cursor_offset(app);
    let line = list_buffer_id.grid_from_offset(app, offset)[1];
    state(app, page_id).matches.get(line).cloned()
}
