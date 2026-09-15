// The changes in one revision, as `jj show <revision>` would show them,
// and a way into the files they are in.
//
// The page renders the diff itself rather than showing a diff tool's
// output, so that every page line's place in the working copy is known
// rather than parsed back out of the text: `locations` says, for each line
// of the page, which file and which line of it the line came from. That is
// what ctrl+enter follows, and what a click on a gutter bar in an edit page
// looks up backwards to scroll this page to the hunk it was pressed on.

use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use bstr::{BString, ByteSlice};

use crate::{
    app::{App, IO, VcsChange, VcsFileKind, VcsLineKind, VcsRevisionId},
    buffer::{self, OffsetDiff},
    drawing::Rect,
    editor::{self, EditorId},
    input::{ButtonState, InputEvent, Key, NamedKey},
    window::WindowId,
};

use super::{GAP, PageContent, PageId, insert, new_edit};

#[derive(Clone)]
pub(super) struct State {
    root: PathBuf,
    revision: VcsRevisionId,
    locations: Vec<Option<Location>>,
    // Where to put the cursor once there is a rendered page to scroll:
    // the hunk a gutter bar was clicked on, or the file and line ctrl+2
    // was pressed on. Cleared by the tick that resolves it.
    reveal: Option<(PathBuf, usize)>,
    // Where ctrl+enter is going, once the revision holding it has been
    // checked out. Only ever set for a revision that is not the working
    // copy: those files are already on disk.
    jump: Option<Location>,
    // What went wrong with the last checkout, for the status bar.
    error: Option<BString>,
}

// Where a line of the page came from.
#[derive(Clone)]
struct Location {
    // Absolute, so it can be compared with a `reveal` and opened as is.
    path: PathBuf,
    // The line in the working-copy file. A removed line maps to the line
    // the deletion sits before.
    line: usize,
    // False for a deleted file: there is nothing to open.
    openable: bool,
}

#[derive(Clone, Copy)]
struct DiffEditors {
    diff_id: EditorId,
    status_bar_id: EditorId,
}

pub(super) const EDITOR_COUNT: usize = 2;

const DIFF_IX: usize = 0;

pub(crate) fn new(
    app: &mut App,
    root: PathBuf,
    revision: VcsRevisionId,
    reveal: Option<(PathBuf, usize)>,
) -> PageId {
    let diff_id = editor::new_generated(app);
    let status_bar_id = editor::new_generated(app);
    insert(
        app,
        PageContent::Diff(State {
            root,
            revision,
            locations: Vec::new(),
            reveal,
            jump: None,
            error: None,
        }),
        vec![diff_id, status_bar_id],
        DIFF_IX,
    )
}

pub(super) fn duplicate(page_id: PageId, app: &mut App, _io: &mut dyn IO) -> PageId {
    let DiffEditors {
        diff_id,
        status_bar_id,
    } = editors(app, page_id);
    let state = state(app, page_id).clone();
    let diff_id = editor::new_copy(app, diff_id);
    let status_bar_id = editor::new_copy(app, status_bar_id);
    insert(
        app,
        PageContent::Diff(state),
        vec![diff_id, status_bar_id],
        DIFF_IX,
    )
}

pub(super) fn tick(page_id: PageId, app: &mut App, io: &mut dyn IO, window_id: WindowId) {
    let DiffEditors {
        diff_id,
        status_bar_id,
    } = editors(app, page_id);
    let root = state(app, page_id).root.clone();
    let revision = state(app, page_id).revision.clone();

    // The change is re-read every frame: the working copy changes under
    // us, from this editor and from anything else running.
    let (text, locations, read) = match io.vcs_change(&root, &revision) {
        Ok(change) => {
            let (text, locations) = render(&change);
            (text, locations, true)
        }
        Err(error) => (BString::from(error.to_string()), Vec::new(), false),
    };

    let buffer_id = app.editors.buffer_id[diff_id];
    if buffer_id.text(app) != text {
        // The page refreshes while it is being read, so hold the cursor
        // on the line it was on rather than sending it back to the top.
        let line = buffer_id.grid_from_offset(app, diff_id.main_cursor_offset(app))[1];
        buffer_id.replace(app, text.as_bstr());
        let offset = buffer_id.offset_from_grid(app, [0, line]);
        diff_id.set_cursor_offsets(app, &[offset]);
    }
    state_mut(app, page_id).locations = locations;

    // A reveal waits for a change to land - the first frames of a page
    // usually have none - and then fires once, whether or not it matched.
    if read && let Some((path, line)) = state(app, page_id).reveal.clone() {
        state_mut(app, page_id).reveal = None;
        if let Some(page_line) = locate(app, page_id, &path, line) {
            let buffer_id = app.editors.buffer_id[diff_id];
            let offset = buffer_id.offset_from_grid(app, [0, page_line]);
            diff_id.set_cursor_offsets(app, &[offset]);
            diff_id.scroll_offset_into_center(app, offset);
        }
    }

    app.editors.gutter_marker[diff_id] = !state(app, page_id).locations.is_empty();
    diff_id.tick(app, io);

    // A jump waits for the revision to be checked out - there is nothing
    // on disk to open until then - and is the last thing this page does.
    let waiting = poll_jump(page_id, app, io, window_id);

    let status_text = status_text(app, page_id, waiting);
    let status_bar_buffer_id = app.editors.buffer_id[status_bar_id];
    status_bar_buffer_id.replace(app, status_text.as_bstr());
    status_bar_id.tick(app, io);
}

// The page is showing a revision, and how it got there.
fn status_text(app: &App, page_id: PageId, waiting: bool) -> BString {
    let state = state(app, page_id);
    if let Some(error) = &state.error {
        return error.clone();
    }
    let name = state.revision.name();
    if waiting {
        BString::from(format!("checking out {name} ..."))
    } else {
        BString::from(format!("jj show {name}"))
    }
}

// Ask for the checkout the pending jump is waiting on, and take the jump
// once it lands. True while it is still running.
fn poll_jump(page_id: PageId, app: &mut App, io: &mut dyn IO, window_id: WindowId) -> bool {
    let state = state(app, page_id);
    let (Some(location), root, revision) = (
        state.jump.clone(),
        state.root.clone(),
        state.revision.clone(),
    ) else {
        return false;
    };
    match io.vcs_checkout(&root, &revision) {
        None => true,
        Some(Ok(())) => {
            state_mut(app, page_id).jump = None;
            open(app, io, window_id, &location);
            false
        }
        Some(Err(error)) => {
            let state = state_mut(app, page_id);
            state.jump = None;
            state.error = Some(BString::from(error.to_string()));
            false
        }
    }
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
    false
}

pub(super) fn layout(page_rect: Rect, cell_size: [u32; 2]) -> Vec<Rect> {
    let [diff_rect, status_bar_rect] = page_rect.split_from_bottom(cell_size[1] as f32, GAP);
    vec![diff_rect, status_bar_rect]
}

pub(super) fn handle_edits(
    _page_id: PageId,
    _app: &mut App,
    _editor_ix: usize,
    _diff: &OffsetDiff,
) {
    // The page is regenerated every tick.
}

pub(super) fn current_path(page_id: PageId, app: &App) -> Option<PathBuf> {
    Some(location(page_id, app)?.path)
}

fn editors(app: &App, page_id: PageId) -> DiffEditors {
    let &[diff_id, status_bar_id] = app.pages.editor_ids[page_id].as_slice() else {
        unreachable!()
    };
    DiffEditors {
        diff_id,
        status_bar_id,
    }
}

fn state(app: &App, page_id: PageId) -> &State {
    let PageContent::Diff(state) = &app.pages.content[page_id] else {
        unreachable!()
    };
    state
}

fn state_mut(app: &mut App, page_id: PageId) -> &mut State {
    let PageContent::Diff(state) = &mut app.pages.content[page_id] else {
        unreachable!()
    };
    state
}

// Where the main cursor's line came from, if it came from anywhere.
fn location(page_id: PageId, app: &App) -> Option<Location> {
    let diff_id = editors(app, page_id).diff_id;
    let buffer_id = app.editors.buffer_id[diff_id];
    let line = buffer_id.grid_from_offset(app, diff_id.main_cursor_offset(app))[1];
    state(app, page_id).locations.get(line)?.clone()
}

// The first page line for `path` at or after `line`, for a reveal.
fn locate(app: &App, page_id: PageId, path: &Path, line: usize) -> Option<usize> {
    state(app, page_id)
        .locations
        .iter()
        .position(|location| {
            location
                .as_ref()
                .is_some_and(|location| location.path == path && location.line >= line)
        })
        // A cursor past the last hunk of the file finds nothing at or
        // after it, so fall back to the file's own header line.
        .or_else(|| {
            state(app, page_id).locations.iter().position(|location| {
                location
                    .as_ref()
                    .is_some_and(|location| location.path == path)
            })
        })
}

fn submit(page_id: PageId, app: &mut App, io: &mut dyn IO, window_id: WindowId) {
    let Some(location) = location(page_id, app) else {
        return;
    };
    if !location.openable {
        return;
    }
    if state(app, page_id).revision == VcsRevisionId::WorkingCopy {
        open(app, io, window_id, &location);
        return;
    }
    // Another revision's files are not the ones on disk, so there is
    // nothing to jump into until it has been checked out. `poll_jump`
    // asks for that, and opens the file once it is there.
    let state = state_mut(app, page_id);
    state.jump = Some(location);
    state.error = None;
}

// Replace this page with the file the location is in, at that line.
fn open(app: &mut App, io: &mut dyn IO, window_id: WindowId, location: &Location) {
    let buffer_id = buffer::from_file(app, io, location.path.clone());
    // Load the file now, so the line can be found in it.
    buffer_id.tick(app, io);
    let editor_id = editor::new(app, buffer_id);
    let offset = buffer_id.offset_from_grid(app, [0, location.line]);
    editor_id.set_cursor_offsets(app, &[offset]);
    editor_id.scroll_offset_into_center(app, offset);
    let page_id = new_edit(app, editor_id);
    window_id.replace_page(app, io, page_id);
}

// Line numbers are this wide, so that the two columns and the diff text
// line up down the page.
const NUMBER_WIDTH: usize = 5;

/// The page this change would be shown on, without the locations that
/// only this page navigates by. The revision picker previews with it, so
/// that what you are choosing between is what you will get.
pub(super) fn render_text(change: &VcsChange) -> BString {
    render(change).0
}

fn render(change: &VcsChange) -> (BString, Vec<Option<Location>>) {
    // One entry per page line, so that a page line's index is its index
    // into `locations`.
    let mut lines: Vec<(BString, Option<Location>)> = Vec::new();
    let mut line = |content: BString, location: Option<Location>| lines.push((content, location));

    line(format!("Change:  {}", change.change_id).into(), None);
    line(format!("Commit:  {}", change.commit_id).into(), None);
    line(format!("Author:  {}", change.author).into(), None);
    line(BString::default(), None);
    if change.description.is_empty() {
        line("    (no description set)".into(), None);
    } else {
        for description in change.description.lines() {
            let mut content = BString::from("    ");
            content.extend_from_slice(description);
            line(content, None);
        }
    }

    for file in &change.files {
        let path = change.root.join(&file.relative_path);
        let openable = file.kind != VcsFileKind::Deleted;
        let at = |line: usize| {
            Some(Location {
                path: path.clone(),
                line,
                openable,
            })
        };

        let kind = match file.kind {
            VcsFileKind::Added => "A",
            VcsFileKind::Deleted => "D",
            VcsFileKind::Modified => "M",
        };
        let mut header = BString::from(format!("{kind} "));
        header.extend_from_slice(file.relative_path.as_os_str().as_bytes());
        if file.binary {
            header.extend_from_slice(b" (binary)");
        }
        line(BString::default(), None);
        // A file with no hunks - a binary one - still opens, at its top.
        let first = file.hunks.first().map_or(0, |hunk| hunk.new_lines.start);
        line(header, at(first));

        for hunk in &file.hunks {
            line(
                format!(
                    "  @@ -{},{} +{},{} @@",
                    number(hunk.old_lines.start, hunk.old_lines.len()),
                    hunk.old_lines.len(),
                    number(hunk.new_lines.start, hunk.new_lines.len()),
                    hunk.new_lines.len()
                )
                .into(),
                at(hunk.new_lines.start),
            );

            // The lines of the hunk, numbered down both sides at once: a
            // removed line has no line in the working copy, an added one
            // has no line in the parent.
            let mut old = hunk.old_lines.start;
            let mut new = hunk.new_lines.start;
            for hunk_line in &hunk.lines {
                let (old_number, new_number, marker) = match hunk_line.kind {
                    VcsLineKind::Context => (Some(old), Some(new), ' '),
                    VcsLineKind::Removed => (Some(old), None, '-'),
                    VcsLineKind::Added => (None, Some(new), '+'),
                };
                let mut content = BString::from(format!(
                    "{:>width$} {:>width$} {marker} ",
                    old_number.map(display_line).unwrap_or_default(),
                    new_number.map(display_line).unwrap_or_default(),
                    width = NUMBER_WIDTH,
                ));
                content.extend_from_slice(&hunk_line.text);
                // A removed line sits before the line it was removed
                // from, so both kinds point at `new`.
                line(content, at(new));
                match hunk_line.kind {
                    VcsLineKind::Context => {
                        old += 1;
                        new += 1;
                    }
                    VcsLineKind::Removed => old += 1,
                    VcsLineKind::Added => new += 1,
                }
            }
        }
    }

    let text = bstr::join("\n", lines.iter().map(|(content, _)| content)).into();
    let locations = lines.into_iter().map(|(_, location)| location).collect();
    (text, locations)
}

fn display_line(line: usize) -> String {
    (line + 1).to_string()
}

// The line number a hunk header shows: 1-based, except that an empty range
// names the line before it, as diff has always done.
fn number(start: usize, len: usize) -> usize {
    if len == 0 { start } else { start + 1 }
}
