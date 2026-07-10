use bstr::{BStr, BString, ByteSlice};
use std::ffi::OsStr;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;

use crate::{
    app::{App, DirEntry, IO},
    buffer::{self, OffsetDiff, Source, SourceFile},
    drawing::{Drawing, Rect},
    editor::{self, EditorId},
    input::{ButtonState, InputEvent, Key, NamedKey},
    map::Map,
    style::{BACKGROUND_COLOR, HIGHLIGHT_COLOR},
    window::WindowId,
};

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

#[derive(Clone, Copy)]
pub enum PageContent {
    Edit,
    OpenFile,
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

pub(crate) fn new_edit(app: &mut App, editor_id: EditorId) -> PageId {
    let status_bar_id = editor::new_scratch(app);
    insert(app, PageContent::Edit, vec![editor_id, status_bar_id], 0)
}

pub(crate) fn new_open_file(app: &mut App, io: &mut dyn IO) -> PageId {
    let preview_id = editor::new_scratch(app);
    let path_id = editor::new_scratch(app);
    let list_id = editor::new_scratch(app);

    // The path editor starts with the current working directory.
    let mut path = io.current_dir().into_os_string().into_vec();
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
        1,
    )
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
        match pages.content[page_id] {
            PageContent::Edit => assert!(editor_ids.len() == 2),
            PageContent::OpenFile => assert!(editor_ids.len() == 3),
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
        match app.pages.content[self] {
            PageContent::Edit => {
                let &[editor_id, status_bar_id] = app.pages.editor_ids[self].as_slice() else {
                    unreachable!()
                };

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
            PageContent::OpenFile => {
                let &[preview_id, path_id, list_id] = app.pages.editor_ids[self].as_slice() else {
                    unreachable!()
                };

                path_id.tick(app, io);

                // Update list text: matching entries, or the listing error.
                let listing = open_file_listing(app, io, path_id);
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

        // OpenFile: enter chords work whichever editor is focused. Plain enter
        // goes to the focused editor like any other key.
        if let PageContent::OpenFile = app.pages.content[self]
            && let InputEvent::Key {
                state: ButtonState::Pressed,
                logical_key: Key::Named(NamedKey::Enter),
            } = event
        {
            match (app.modifiers.control, app.modifiers.alt) {
                (true, false) => {
                    self.open_file_submit(app, io, window_id);
                    return;
                }
                (false, true) => {
                    self.open_file_create(app, io, window_id);
                    return;
                }
                _ => {}
            }
        }

        // OpenFile: ctrl+i/k always move the list cursor, so the selection
        // can be changed while typing in the path editor.
        if let PageContent::OpenFile = app.pages.content[self]
            && let InputEvent::Key {
                state: ButtonState::Pressed,
                logical_key: Key::Character("i" | "k"),
            } = event
            && app.modifiers.control
            && !app.modifiers.alt
        {
            let list_id = app.pages.editor_ids[self][2];
            list_id.input(app, io, event);
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
            app.pages.editor_rects[self] = match app.pages.content[self] {
                PageContent::Edit => {
                    let [editor_rect, status_bar_rect] =
                        page_rect.split_from_bottom(cell_size[1] as f32, GAP);
                    vec![editor_rect, status_bar_rect]
                }
                PageContent::OpenFile => {
                    let [preview_rect, rest] =
                        page_rect.split_from_bottom(page_rect.size[1] / 2.0, GAP);
                    let [path_rect, list_rect] = rest.split_from_top(cell_size[1] as f32, GAP);
                    vec![preview_rect, path_rect, list_rect]
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

    pub(crate) fn handle_edits(self, app: &mut App, _editor_ix: usize, _diff: &OffsetDiff) {
        match app.pages.content[self] {
            PageContent::Edit => {}
            // The list and preview are recomputed every tick.
            PageContent::OpenFile => {}
        }
    }

    // Descend into the selected dir, or open the selected file, replacing
    // this window's page.
    fn open_file_submit(self, app: &mut App, io: &mut dyn IO, window_id: WindowId) {
        let &[_, path_id, list_id] = app.pages.editor_ids[self].as_slice() else {
            unreachable!()
        };
        let Ok(listing) = open_file_listing(app, io, path_id) else {
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
            open_edit_in_window(app, window_id, path);
        }
    }

    // Create the file at the entered path, and any missing parent dirs,
    // then open it, replacing this window's page.
    fn open_file_create(self, app: &mut App, io: &mut dyn IO, window_id: WindowId) {
        let &[_, path_id, _] = app.pages.editor_ids[self].as_slice() else {
            unreachable!()
        };
        let path_buffer_id = app.editors.buffer_id[path_id];
        let text = path_buffer_id.text(app);
        // No file name to create.
        if text.is_empty() || text.last() == Some(&b'/') {
            return;
        }
        let path = PathBuf::from(OsStr::from_bytes(text));
        if io.file_create(&path).is_err() {
            return;
        }
        open_edit_in_window(app, window_id, path);
    }
}

// Show the file at `path` in the window, replacing its page.
fn open_edit_in_window(app: &mut App, window_id: WindowId, path: PathBuf) {
    let buffer_id = buffer::from_file(app, path);
    let editor_id = editor::new(app, buffer_id);
    app.windows.page_id[window_id] = new_edit(app, editor_id);
}

const PREVIEW_BYTES: usize = 10 * 1024;

struct Listing {
    dir: PathBuf,
    // Filtered by fuzzy match against the non-dir part of the path,
    // sorted by closest match.
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

fn open_file_listing(app: &App, io: &mut dyn IO, path_id: EditorId) -> Result<Listing, String> {
    let path_buffer_id = app.editors.buffer_id[path_id];
    let text = path_buffer_id.text(app);
    // The dir part is everything up to and including the last '/'; the rest
    // is the fuzzy match pattern.
    let split = text.rfind_byte(b'/').map_or(0, |ix| ix + 1);
    let (dir, pattern) = text.split_at(split);
    let dir = PathBuf::from(OsStr::from_bytes(dir));
    let mut entries = io
        .dir_list(&dir)
        .map_err(|error| format!("{}: {}", dir.display(), error))?;
    let mut scored: Vec<(FuzzyScore, DirEntry)> = entries
        .drain(..)
        .filter_map(|entry| Some((fuzzy_score(entry.name.as_bytes(), pattern)?, entry)))
        .collect();
    scored.sort_by(|(score_a, entry_a), (score_b, entry_b)| {
        score_a
            .cmp(score_b)
            .then_with(|| entry_a.name.cmp(&entry_b.name))
    });
    let entries = scored.into_iter().map(|(_, entry)| entry).collect();
    Ok(Listing { dir, entries })
}

// Lower is better: the tightest match wins, ties broken by earliest match
// then shortest name.
type FuzzyScore = [usize; 3];

// Case-insensitive subsequence match of `pattern` against `name`.
fn fuzzy_score(name: &[u8], pattern: &[u8]) -> Option<FuzzyScore> {
    // An empty pattern ties everything, leaving the list sorted by name.
    if pattern.is_empty() {
        return Some([0, 0, 0]);
    }
    let lower = |byte: u8| byte.to_ascii_lowercase();

    // Forward pass: earliest end of a subsequence match.
    let mut pattern_ix = 0;
    let mut end = 0;
    for (ix, &byte) in name.iter().enumerate() {
        if lower(byte) == lower(pattern[pattern_ix]) {
            pattern_ix += 1;
            if pattern_ix == pattern.len() {
                end = ix;
                break;
            }
        }
    }
    if pattern_ix < pattern.len() {
        return None;
    }

    // Backward pass: latest start of a match ending at `end`.
    let mut pattern_ix = pattern.len();
    let mut start = end;
    for ix in (0..=end).rev() {
        if lower(name[ix]) == lower(pattern[pattern_ix - 1]) {
            pattern_ix -= 1;
            if pattern_ix == 0 {
                start = ix;
                break;
            }
        }
    }

    Some([end - start, start, name.len()])
}
