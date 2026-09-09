use std::ffi::OsStr;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;

use bstr::{BStr, BString, ByteSlice};

use crate::{
    app::{App, DirEntry, IO},
    buffer::{self, OffsetDiff},
    drawing::Rect,
    editor::{self, EditorId},
    fuzzy,
    input::{ButtonState, InputEvent, Key, NamedKey},
    window::WindowId,
};

use super::{GAP, PageContent, PageId, insert, new_edit};

#[derive(Clone, Copy)]
struct OpenFileEditors {
    preview_id: EditorId,
    path_id: EditorId,
    list_id: EditorId,
}

pub(super) const EDITOR_COUNT: usize = 3;

const PATH_IX: usize = 1;
const PREVIEW_BYTES: usize = 10 * 1024;

pub(crate) fn new(app: &mut App, dir: PathBuf) -> PageId {
    let preview_id = editor::new_generated(app);
    let path_id = editor::new_scratch(app);
    let list_id = editor::new_generated(app);

    // The path editor starts with the provided directory.
    let mut path = dir.into_os_string().into_vec();
    if path.last() != Some(&b'/') {
        path.push(b'/');
    }
    let path_buffer_id = app.editors.buffer_id[path_id];
    path_buffer_id.replace(app, BStr::new(&path));
    path_id.cursor_goto_buffer_end(app);

    insert(
        app,
        PageContent::OpenFile,
        vec![preview_id, path_id, list_id],
        PATH_IX,
    )
}

pub(super) fn duplicate(page_id: PageId, app: &mut App, _io: &mut dyn IO) -> PageId {
    let OpenFileEditors {
        preview_id,
        path_id,
        list_id,
    } = editors(app, page_id);
    let preview_id = editor::new_copy(app, preview_id);
    let path_id = editor::new_copy(app, path_id);
    let list_id = editor::new_copy(app, list_id);
    let focus = app.pages.focus[page_id];
    insert(
        app,
        PageContent::OpenFile,
        vec![preview_id, path_id, list_id],
        focus,
    )
}

pub(super) fn tick(page_id: PageId, app: &mut App, io: &mut dyn IO) {
    let OpenFileEditors {
        preview_id,
        path_id,
        list_id,
    } = editors(app, page_id);

    path_id.tick(app, io);

    // Update list text: matching entries, or the listing error.
    let listing = listing(app, io, path_id);
    let list_text = match &listing {
        Ok(listing) => {
            let lines = listing.entries.iter().map(|entry| {
                let mut line = BString::from(entry.name.as_bytes());
                if entry.is_dir {
                    line.push(b'/');
                }
                line
            });
            bstr::join("\n", lines).into()
        }
        Err(error) => BString::from(error.clone()),
    };
    let list_buffer_id = app.editors.buffer_id[list_id];
    if list_buffer_id.text(app) != list_text.as_bstr() {
        list_buffer_id.replace(app, list_text.as_bstr());
        // The list changed, so select the closest match again.
        list_id.cursor_reset(app);
    }

    // Mark the selected line, if there is one.
    app.editors.gutter_marker[list_id] =
        matches!(&listing, Ok(listing) if !listing.entries.is_empty());

    // Update preview text: the start of the selected file, if any.
    let preview_text = match &listing {
        Ok(listing) => match listing.selected(app, list_id) {
            Some(entry) if !entry.is_dir => {
                let path = listing.dir.join(&entry.name);
                match io.file_read_prefix(&path, PREVIEW_BYTES) {
                    Ok(contents) => BString::from(contents),
                    Err(error) => BString::from(error.to_string()),
                }
            }
            _ => BString::default(),
        },
        Err(_) => BString::default(),
    };
    let preview_buffer_id = app.editors.buffer_id[preview_id];
    if preview_buffer_id.text(app) != preview_text.as_bstr() {
        preview_buffer_id.replace(app, preview_text.as_bstr());
        // The selection changed, so show the top of the new file.
        preview_id.cursor_reset(app);
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
                submit(page_id, app, io, window_id);
                return true;
            }
            (false, true) => {
                create(page_id, app, io, window_id);
                return true;
            }
            _ => {}
        }
    }

    // Ctrl+i/k always move the list cursor, so the selection can be changed
    // while typing in the path editor.
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
    let [path_rect, list_rect] = rest.split_from_top(cell_size[1] as f32, GAP);
    vec![preview_rect, path_rect, list_rect]
}

pub(super) fn handle_edits(
    _page_id: PageId,
    _app: &mut App,
    _editor_ix: usize,
    _diff: &OffsetDiff,
) {
    // The list and preview are recomputed every tick.
}

pub(super) fn current_path(_page_id: PageId, _app: &App) -> Option<PathBuf> {
    None
}

fn editors(app: &App, page_id: PageId) -> OpenFileEditors {
    let &[preview_id, path_id, list_id] = app.pages.editor_ids[page_id].as_slice() else {
        unreachable!()
    };
    OpenFileEditors {
        preview_id,
        path_id,
        list_id,
    }
}

// Descend into the selected dir, or replace this picker with the selected file.
fn submit(page_id: PageId, app: &mut App, io: &mut dyn IO, window_id: WindowId) {
    let OpenFileEditors {
        path_id, list_id, ..
    } = editors(app, page_id);
    let Ok(listing) = listing(app, io, path_id) else {
        return;
    };
    let Some(entry) = listing.selected(app, list_id) else {
        return;
    };
    if entry.is_dir {
        let mut path = listing.dir.as_os_str().as_bytes().to_vec();
        path.extend_from_slice(entry.name.as_bytes());
        path.push(b'/');
        let path_buffer_id = app.editors.buffer_id[path_id];
        path_buffer_id.replace(app, BStr::new(&path));
        path_id.cursor_goto_buffer_end(app);
    } else {
        let path = listing.dir.join(&entry.name);
        open_edit_in_window(app, io, window_id, path);
    }
}

// Create the file at the entered path, and any missing parent dirs, then
// replace this picker with it.
fn create(page_id: PageId, app: &mut App, io: &mut dyn IO, window_id: WindowId) {
    let path_id = editors(app, page_id).path_id;
    let path_buffer_id = app.editors.buffer_id[path_id];
    let text = path_buffer_id.text(app);
    // No file name to create.
    if text.is_empty() || text.last() == Some(&b'/') {
        return;
    }
    let path = PathBuf::from(OsStr::from_bytes(text));
    // Buffers need absolute paths; a relative one would be created and
    // opened relative to wherever the editor was started.
    if !path.is_absolute() {
        return;
    }
    if io.file_create(&path).is_err() {
        return;
    }
    open_edit_in_window(app, io, window_id, path);
}

// Replace this picker with the file at `path`.
fn open_edit_in_window(app: &mut App, io: &mut dyn IO, window_id: WindowId, path: PathBuf) {
    let buffer_id = buffer::from_file(app, path);
    let editor_id = editor::new(app, buffer_id);
    let page_id = new_edit(app, editor_id);
    window_id.replace_page(app, io, page_id);
}

struct Listing {
    dir: PathBuf,
    // Filtered by fuzzy match against the non-dir part of the path, sorted by
    // closest match.
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
    // The dir part is everything up to and including the last '/'; the rest is
    // the fuzzy match pattern.
    let split = text.rfind_byte(b'/').map_or(0, |ix| ix + 1);
    let (dir, pattern) = text.split_at(split);
    let dir = PathBuf::from(OsStr::from_bytes(dir));
    if !dir.is_absolute() {
        return Err(format!("{}: not an absolute path", dir.display()));
    }
    let mut entries = io
        .dir_list(&dir)
        .map_err(|error| format!("{}: {}", dir.display(), error))?;
    let mut scored: Vec<(fuzzy::Score, DirEntry)> = entries
        .drain(..)
        .filter_map(|entry| Some((fuzzy::score(entry.name.as_bytes(), pattern)?, entry)))
        .collect();
    scored.sort_by(|(score_a, entry_a), (score_b, entry_b)| {
        score_a
            .cmp(score_b)
            .then_with(|| entry_a.name.cmp(&entry_b.name))
    });
    let entries = scored.into_iter().map(|(_, entry)| entry).collect();
    Ok(Listing { dir, entries })
}
