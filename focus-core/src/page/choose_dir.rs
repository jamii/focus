use std::ffi::OsStr;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;

use bstr::{BStr, BString, ByteSlice};

use crate::{
    app::{App, DirEntry, IO},
    buffer::OffsetDiff,
    drawing::Rect,
    editor::{self, EditorId},
    input::{ButtonState, InputEvent, Key, NamedKey},
    window::WindowId,
};

use super::{GAP, PageContent, PageId, insert, new_choose_command};

#[derive(Clone, Copy)]
struct ChooseDirEditors {
    list_id: EditorId,
    path_id: EditorId,
}

pub(super) const EDITOR_COUNT: usize = 2;

const PATH_IX: usize = 0;

pub(crate) fn new(app: &mut App, _io: &mut dyn IO, dir: PathBuf) -> PageId {
    let path_id = editor::new_scratch(app);
    let list_id = editor::new_generated(app);

    // The path editor starts with the provided directory (with trailing `/`).
    let mut path = dir.into_os_string().into_vec();
    if path.last() != Some(&b'/') {
        path.push(b'/');
    }
    let path_buffer_id = app.editors.buffer_id[path_id];
    path_buffer_id.replace(app, BStr::new(&path));
    path_id.cursor_goto_buffer_end(app);

    insert(app, PageContent::ChooseDir, vec![path_id, list_id], PATH_IX)
}

pub(super) fn duplicate(page_id: PageId, app: &mut App, _io: &mut dyn IO) -> PageId {
    let ChooseDirEditors { path_id, list_id } = editors(app, page_id);
    let path_id = editor::new_copy(app, path_id);
    let list_id = editor::new_copy(app, list_id);
    let focus = app.pages.focus[page_id];
    insert(app, PageContent::ChooseDir, vec![path_id, list_id], focus)
}

pub(super) fn tick(page_id: PageId, app: &mut App, io: &mut dyn IO) {
    let ChooseDirEditors { path_id, list_id } = editors(app, page_id);

    path_id.tick(app, io);

    // Update list text: directory entries only (dirs have trailing `/`).
    let listing = listing(app, io, path_id);
    let list_text: BString = match &listing {
        Ok(listing) => {
            let lines = listing.entries.iter().map(|entry| {
                let mut line = BString::from(entry.name.as_bytes());
                line.push(b'/');
                line
            });
            bstr::join("\n", lines).into()
        }
        Err(error) => BString::from(error.clone()),
    };
    let list_buffer_id = app.editors.buffer_id[list_id];
    if list_buffer_id.text(app) != list_text.as_bstr() {
        list_buffer_id.replace(app, list_text.as_bstr());
        list_id.cursor_reset(app);
    }

    // Mark the selected line if there are entries.
    app.editors.gutter_marker[list_id] =
        matches!(&listing, Ok(listing) if !listing.entries.is_empty());

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
    {
        match (app.modifiers.control, app.modifiers.alt) {
            (true, false) => {
                descend(page_id, app, io);
                return true;
            }
            (false, true) => {
                accept(page_id, app, io, window_id);
                return true;
            }
            _ => {}
        }
    }

    // Ctrl+i/k always move the list cursor.
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
    let [path_rect, list_rect] = page_rect.split_from_top(cell_size[1] as f32, GAP);
    vec![path_rect, list_rect]
}

pub(super) fn handle_edits(
    _page_id: PageId,
    _app: &mut App,
    _editor_ix: usize,
    _diff: &OffsetDiff,
) {
    // The list is recomputed every tick.
}

pub(super) fn current_path(_page_id: PageId, _app: &App) -> Option<PathBuf> {
    None
}

fn editors(app: &App, page_id: PageId) -> ChooseDirEditors {
    let &[path_id, list_id] = app.pages.editor_ids[page_id].as_slice() else {
        unreachable!()
    };
    ChooseDirEditors { path_id, list_id }
}

// Ctrl+Enter: descend into the selected directory entry.
fn descend(page_id: PageId, app: &mut App, io: &mut dyn IO) {
    let ChooseDirEditors { path_id, list_id } = editors(app, page_id);
    let Ok(listing) = listing(app, io, path_id) else {
        return;
    };
    let Some(entry) = listing.selected(app, list_id) else {
        return;
    };
    // Replace the path text with the listed dir plus the selected entry,
    // dropping any filter typed after the last `/`.
    let mut path = listing.dir.as_os_str().as_bytes().to_vec();
    path.extend_from_slice(entry.name.as_bytes());
    path.push(b'/');
    let path_buffer_id = app.editors.buffer_id[path_id];
    path_buffer_id.replace(app, BStr::new(&path));
    path_id.cursor_goto_buffer_end(app);
}

// Alt+Enter: accept the whole path text as the dir to run in, and replace
// this page with the command picker.
fn accept(page_id: PageId, app: &mut App, io: &mut dyn IO, window_id: WindowId) {
    let path_id = editors(app, page_id).path_id;
    let path_buffer_id = app.editors.buffer_id[path_id];
    let dir = PathBuf::from(OsStr::from_bytes(path_buffer_id.text(app)));
    // The command runs here, and locations in its output resolve against
    // here, so take the text only when it names a directory that exists.
    // Accepting the text up to the last `/` instead would quietly run
    // `/foo` in `/`, and a relative dir would run wherever the editor was
    // started. Use ctrl+enter to descend into a filtered entry.
    if !dir.is_absolute() || io.dir_list(&dir).is_err() {
        return;
    }
    let command_page_id = new_choose_command(app, io, dir);
    window_id.replace_page(app, io, command_page_id);
}

struct Listing {
    // The dir portion of the path text: up to and including the last `/`.
    dir: PathBuf,
    // Subdirectories of `dir`, filtered by exact substring match against the
    // rest of the path text, in the order returned by dir_list.
    entries: Vec<DirEntry>,
}

impl Listing {
    // The entry on the same line as the list editor's main cursor.
    fn selected<'a>(&'a self, app: &App, list_id: EditorId) -> Option<&'a DirEntry> {
        let list_buffer_id = app.editors.buffer_id[list_id];
        let offset = app.editors.cursors[list_id].last().unwrap().head.offset;
        let line = list_buffer_id.grid_from_offset(app, offset)[1];
        self.entries.get(line)
    }
}

fn listing(app: &App, io: &mut dyn IO, path_id: EditorId) -> Result<Listing, String> {
    let path_buffer_id = app.editors.buffer_id[path_id];
    let text = path_buffer_id.text(app);
    let split = text.rfind_byte(b'/').map_or(0, |ix| ix + 1);
    let (dir_bytes, pattern) = text.split_at(split);
    let dir = PathBuf::from(OsStr::from_bytes(dir_bytes));
    if !dir.is_absolute() {
        return Err(format!("{}: not an absolute path", dir.display()));
    }
    let entries = io
        .dir_list(&dir)
        .map_err(|error| format!("{}: {}", dir.display(), error))?;
    let entries = entries
        .into_iter()
        .filter(|entry| {
            entry.is_dir
                && (pattern.is_empty()
                    || entry
                        .name
                        .as_bytes()
                        .windows(pattern.len())
                        .any(|w| w == pattern))
        })
        .collect();
    Ok(Listing { dir, entries })
}
