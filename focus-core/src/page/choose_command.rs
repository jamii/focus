use std::collections::HashSet;
use std::path::{Path, PathBuf};

use bstr::{BStr, BString, ByteSlice};

use crate::{
    app::{App, IO},
    buffer::OffsetDiff,
    drawing::Rect,
    editor::{self, EditorId},
    input::{ButtonState, InputEvent, Key, NamedKey},
    window::WindowId,
};

use super::{GAP, PageContent, PageId, insert, new_runner};

#[derive(Clone)]
pub(super) struct State {
    dir: PathBuf,
    history: Vec<CommandEntry>,
    matches: Vec<CommandEntry>,
    list_text: BString,
    last_pattern: Option<BString>,
}

// A shell command from fish history. `display` is fish's own escaped form
// of it, taken straight from the history file, so each entry is exactly one
// line in the list.
#[derive(Clone, PartialEq, Eq)]
struct CommandEntry {
    command: BString,
    display: BString,
}

#[derive(Clone, Copy)]
struct ChooseCommandEditors {
    preview_id: EditorId,
    search_id: EditorId,
    list_id: EditorId,
}

pub(super) const EDITOR_COUNT: usize = 3;

const SEARCH_IX: usize = 1;

pub(crate) fn new(app: &mut App, io: &mut dyn IO, dir: PathBuf) -> PageId {
    let preview_id = editor::new_scratch(app);
    let search_id = editor::new_scratch(app);
    let list_id = editor::new_scratch(app);

    insert(
        app,
        PageContent::ChooseCommand(State {
            dir,
            history: load_history(io),
            matches: Vec::new(),
            list_text: BString::default(),
            last_pattern: None,
        }),
        vec![preview_id, search_id, list_id],
        SEARCH_IX,
    )
}

pub(super) fn tick(page_id: PageId, app: &mut App, io: &mut dyn IO) {
    let ChooseCommandEditors {
        preview_id,
        search_id,
        list_id,
    } = editors(app, page_id);

    search_id.tick(app, io);

    // Update list text: matching history entries.
    refresh_matches(app, page_id, search_id);
    let list_buffer_id = app.editors.buffer_id[list_id];
    let list_changed = {
        let list_text = &state(app, page_id).list_text;
        list_buffer_id.text(app) != list_text.as_bstr()
    };
    if list_changed {
        let list_text = state(app, page_id).list_text.clone();
        list_buffer_id.reset(app, list_text.as_bstr());
        // The list changed, so select the closest match again.
        list_id.cursor_reset(app);
    }

    // Mark the selected line, if there is one.
    app.editors.gutter_marker[list_id] = has_selection(app, page_id);

    // Leave the preview blank.
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
                submit_raw(page_id, app, io, window_id);
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
    // The list is refreshed during tick.
}

pub(super) fn current_path(_page_id: PageId, _app: &App) -> Option<PathBuf> {
    None
}

fn editors(app: &App, page_id: PageId) -> ChooseCommandEditors {
    let &[preview_id, search_id, list_id] = app.pages.editor_ids[page_id].as_slice() else {
        unreachable!()
    };
    ChooseCommandEditors {
        preview_id,
        search_id,
        list_id,
    }
}

fn state(app: &App, page_id: PageId) -> &State {
    let PageContent::ChooseCommand(state) = &app.pages.content[page_id] else {
        unreachable!()
    };
    state
}

fn state_mut(app: &mut App, page_id: PageId) -> &mut State {
    let PageContent::ChooseCommand(state) = &mut app.pages.content[page_id] else {
        unreachable!()
    };
    state
}

// Run the selected history entry.
fn submit_selected(page_id: PageId, app: &mut App, io: &mut dyn IO, window_id: WindowId) {
    let ChooseCommandEditors {
        search_id, list_id, ..
    } = editors(app, page_id);
    refresh_matches(app, page_id, search_id);
    let Some(command) = selected_command(app, page_id, list_id) else {
        return;
    };
    run(page_id, app, io, window_id, command);
}

// Run whatever is typed in the search editor.
fn submit_raw(page_id: PageId, app: &mut App, io: &mut dyn IO, window_id: WindowId) {
    let search_id = editors(app, page_id).search_id;
    let search_buffer_id = app.editors.buffer_id[search_id];
    let command = BString::from(search_buffer_id.text(app).to_vec());
    run(page_id, app, io, window_id, command);
}

// Replace this picker with a runner page running `command` at the chosen dir.
fn run(page_id: PageId, app: &mut App, io: &mut dyn IO, window_id: WindowId, command: BString) {
    if command.is_empty() {
        return;
    }
    let dir = state(app, page_id).dir.clone();
    append_history(io, &dir, command.as_bstr());
    let runner_page_id = new_runner(app, io, dir, command);
    window_id.replace_page(app, io, runner_page_id);
}

fn refresh_matches(app: &mut App, page_id: PageId, search_id: EditorId) {
    let search_buffer_id = app.editors.buffer_id[search_id];
    let pattern = BString::from(search_buffer_id.text(app).to_vec());
    let state = state_mut(app, page_id);
    if state.last_pattern.as_ref() == Some(&pattern) {
        return;
    }
    state.last_pattern = Some(pattern.clone());
    state.matches.clear();
    // Exact case-sensitive substring match; history is already most-recent-first,
    // so preserve that order (no re-sorting).
    state.matches = state
        .history
        .iter()
        .filter(|entry| {
            pattern.is_empty()
                || entry
                    .display
                    .windows(pattern.len())
                    .any(|w| w == pattern.as_slice())
        })
        .cloned()
        .collect();
    state.list_text = matches_text(&state.matches);
}

fn matches_text(matches: &[CommandEntry]) -> BString {
    let lines = matches.iter().map(|entry| entry.display.clone());
    bstr::join("\n", lines).into()
}

fn has_selection(app: &App, page_id: PageId) -> bool {
    !state(app, page_id).matches.is_empty()
}

// The command on the same line as the list editor's main cursor.
fn selected_command(app: &App, page_id: PageId, list_id: EditorId) -> Option<BString> {
    let list_buffer_id = app.editors.buffer_id[list_id];
    let offset = app.editors.cursors[list_id].last().unwrap().head.offset;
    let line = list_buffer_id.grid_from_offset(app, offset)[1];
    let matches = &state(app, page_id).matches;
    Some(matches.get(line)?.command.clone())
}

fn fish_history_path(io: &mut dyn IO) -> PathBuf {
    io.home_dir().join(".local/share/fish/fish_history")
}

// Fish history entries, most recent first, deduped. An unreadable or
// unparseable history file gives an empty history rather than an error.
fn load_history(io: &mut dyn IO) -> Vec<CommandEntry> {
    let path = fish_history_path(io);
    let contents = io.file_read(&path).unwrap_or_default();

    // The file is chronological oldest-first: entries start with "- cmd: ",
    // other lines ("  when: ...", "  paths:", ...) are metadata.
    let mut entries = Vec::new();
    for line in contents.lines() {
        if let Some(cmd) = line.strip_prefix(b"- cmd: ") {
            // The line is already the escaped one-line form, so it is the
            // display text; only the command needs unescaping.
            entries.push(CommandEntry {
                command: unescape_cmd(cmd),
                display: cmd.into(),
            });
        }
    }

    // Most recent first, keeping only the most recent occurrence.
    let mut seen: HashSet<BString> = HashSet::new();
    entries
        .into_iter()
        .rev()
        .filter(|entry| seen.insert(entry.command.clone()))
        .collect()
}

// Record the command in fish's history by asking fish to do it. The entry
// format, the escaping and the timestamp are all fish's business, and
// `fish --command` records nothing on its own, so this is the one thing we
// have to ask for. The command text goes across as an argument rather than
// interpolated into the script, so it needs no quoting: the first `--`
// stops fish reading it as an option, the second stops `history append`
// doing the same.
fn append_history(io: &mut dyn IO, dir: &Path, command: &BStr) {
    io.process_spawn_detached(dir, BStr::new("history append -- $argv[1]"), &[command]);
}

// Undo fish's escaping of newlines and backslashes in cmd text.
fn unescape_cmd(cmd: &[u8]) -> BString {
    let mut out = Vec::with_capacity(cmd.len());
    let mut i = 0;
    while i < cmd.len() {
        match (cmd[i], cmd.get(i + 1)) {
            (b'\\', Some(&b'n')) => {
                out.push(b'\n');
                i += 2;
            }
            (b'\\', Some(&b'\\')) => {
                out.push(b'\\');
                i += 2;
            }
            (byte, _) => {
                out.push(byte);
                i += 1;
            }
        }
    }
    out.into()
}
