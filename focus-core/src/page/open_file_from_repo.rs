use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use bstr::{BStr, BString, ByteSlice};

use crate::{
    app::{App, IO, RepoFiles},
    buffer::{self, OffsetDiff},
    drawing::Rect,
    editor::{self, EditorId},
    fuzzy,
    input::{ButtonState, InputEvent, Key, NamedKey},
    window::WindowId,
};

use super::{GAP, PageContent, PageId, insert, new_edit};

#[derive(Clone)]
pub(super) struct State {
    root: PathBuf,
    files: Result<Vec<RepoFile>, String>,
}

#[derive(Clone)]
struct RepoFile {
    relative_path: PathBuf,
    display: BString,
}

#[derive(Clone, Copy)]
struct OpenFileFromRepoEditors {
    preview_id: EditorId,
    search_id: EditorId,
    list_id: EditorId,
}

pub(super) const EDITOR_COUNT: usize = 3;

const SEARCH_IX: usize = 1;
const PREVIEW_BYTES: usize = 10 * 1024;

pub(crate) fn new(app: &mut App, io: &mut dyn IO, dir: PathBuf) -> PageId {
    let state = match io.repo_files(&dir) {
        Ok(RepoFiles {
            root,
            relative_paths,
        }) => State {
            root,
            files: Ok(relative_paths.into_iter().map(repo_file).collect()),
        },
        Err(error) => State {
            root: dir.clone(),
            files: Err(format!("{}: {}", dir.display(), error)),
        },
    };

    let preview_id = editor::new_scratch(app);
    let search_id = editor::new_scratch(app);
    let list_id = editor::new_scratch(app);

    insert(
        app,
        PageContent::OpenFileFromRepo(state),
        vec![preview_id, search_id, list_id],
        SEARCH_IX,
    )
}

pub(super) fn tick(page_id: PageId, app: &mut App, io: &mut dyn IO) {
    let OpenFileFromRepoEditors {
        preview_id,
        search_id,
        list_id,
    } = editors(app, page_id);

    search_id.tick(app, io);

    // Update list text: matching repo files, or the listing error captured when
    // the page was opened.
    let listing = listing(app, page_id, search_id);
    let list_text = match &listing {
        Ok(listing) => {
            let lines = listing.entries.iter().map(|entry| entry.display.clone());
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
            Some(entry) => {
                let path = listing.root.join(&entry.relative_path);
                match io.file_read_prefix(&path, PREVIEW_BYTES) {
                    Ok(contents) => BString::from(contents),
                    Err(error) => BString::from(error.to_string()),
                }
            }
            None => BString::default(),
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
    // Ctrl+enter opens the selected file. Plain enter goes to the focused
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
    // The list and preview are recomputed every tick.
}

pub(super) fn current_path(_page_id: PageId, _app: &App) -> Option<PathBuf> {
    None
}

fn editors(app: &App, page_id: PageId) -> OpenFileFromRepoEditors {
    let &[preview_id, search_id, list_id] = app.pages.editor_ids[page_id].as_slice() else {
        unreachable!()
    };
    OpenFileFromRepoEditors {
        preview_id,
        search_id,
        list_id,
    }
}

fn state(app: &App, page_id: PageId) -> &State {
    let PageContent::OpenFileFromRepo(state) = &app.pages.content[page_id] else {
        unreachable!()
    };
    state
}

// Open the selected file, replacing this window's page.
fn submit(page_id: PageId, app: &mut App, _io: &mut dyn IO, window_id: WindowId) {
    let OpenFileFromRepoEditors {
        search_id, list_id, ..
    } = editors(app, page_id);
    let Ok(listing) = listing(app, page_id, search_id) else {
        return;
    };
    let Some(entry) = listing.selected(app, list_id) else {
        return;
    };
    let path = listing.root.join(&entry.relative_path);
    open_edit_in_window(app, window_id, path);
}

// Show the file at `path` in the window, replacing its page.
fn open_edit_in_window(app: &mut App, window_id: WindowId, path: PathBuf) {
    let buffer_id = buffer::from_file(app, path);
    let editor_id = editor::new(app, buffer_id);
    app.windows.page_id[window_id] = new_edit(app, editor_id);
}

struct Listing {
    root: PathBuf,
    // Filtered by fuzzy match against the full relative path, sorted by closest
    // match.
    entries: Vec<RepoFile>,
}

impl Listing {
    // The entry on the same line as the list editor's main cursor.
    fn selected<'a>(&'a self, app: &App, list_id: EditorId) -> Option<&'a RepoFile> {
        let list_buffer_id = app.editors.buffer_id[list_id];
        let offset = app.editors.cursors[list_id].last().unwrap().head.offset;
        let line = list_buffer_id.grid_from_offset(app, offset)[1];
        self.entries.get(line)
    }
}

fn listing(app: &App, page_id: PageId, search_id: EditorId) -> Result<Listing, String> {
    let state = state(app, page_id);
    let files = state.files.as_ref().map_err(Clone::clone)?;
    let search_buffer_id = app.editors.buffer_id[search_id];
    let pattern = search_buffer_id.text(app);
    let mut scored: Vec<(fuzzy::Score, RepoFile)> = files
        .iter()
        .filter_map(|entry| Some((fuzzy::score(&entry.display, pattern)?, entry.clone())))
        .collect();
    scored.sort_by(|(score_a, entry_a), (score_b, entry_b)| {
        score_a
            .cmp(score_b)
            .then_with(|| entry_a.display.cmp(&entry_b.display))
    });
    let entries = scored.into_iter().map(|(_, entry)| entry).collect();
    Ok(Listing {
        root: state.root.clone(),
        entries,
    })
}

fn repo_file(relative_path: PathBuf) -> RepoFile {
    RepoFile {
        display: path_bytes(&relative_path),
        relative_path,
    }
}

fn path_bytes(path: &Path) -> BString {
    BStr::new(path.as_os_str().as_bytes()).into()
}
