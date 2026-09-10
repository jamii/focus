use std::mem::take;
use std::path::{Path, PathBuf};

use bstr::{BStr, BString, ByteSlice};

use crate::{
    app::{App, IO, ProcessId},
    buffer::OffsetDiff,
    drawing::Rect,
    editor::{self, EditorId},
    input::{ButtonState, InputEvent, Key, NamedKey},
    window::WindowId,
};

use super::{GAP, PageContent, PageId, insert};

#[derive(Clone)]
pub(super) struct State {
    dir: PathBuf,
    loading: Option<Loading>,
    commands: Vec<BString>,
    matches: Vec<BString>,
    list_text: BString,
    last_pattern: Option<BString>,
}

// The completion process and its output so far. The output arrives over
// several frames, so it is accumulated until the process exits.
#[derive(Clone)]
struct Loading {
    process_id: ProcessId,
    output: Vec<u8>,
}

#[derive(Clone, Copy)]
struct LauncherEditors {
    search_id: EditorId,
    list_id: EditorId,
}

pub(super) const EDITOR_COUNT: usize = 2;

const SEARCH_IX: usize = 0;
const COMPLETE_COMMAND: &str = "complete -C ''";
const LAUNCH_COMMAND: &str = "eval \"$argv[1] &\"; disown";

pub(crate) fn new(app: &mut App, io: &mut dyn IO) -> PageId {
    let dir = io.home_dir();
    let loading = spawn_load(io, &dir);
    let search_id = editor::new_scratch(app);
    let list_id = editor::new_generated(app);

    insert(
        app,
        PageContent::Launcher(State {
            dir,
            loading: Some(loading),
            commands: Vec::new(),
            matches: Vec::new(),
            list_text: BString::default(),
            last_pattern: None,
        }),
        vec![search_id, list_id],
        SEARCH_IX,
    )
}

pub(super) fn duplicate(page_id: PageId, app: &mut App, io: &mut dyn IO) -> PageId {
    let LauncherEditors { search_id, list_id } = editors(app, page_id);
    let mut state = state(app, page_id).clone();
    // A process can only be drained by one page - process_poll hands each
    // chunk of output to a single caller - so a copy made while the command
    // list is still loading runs its own completion process.
    if state.loading.is_some() {
        state.loading = Some(spawn_load(io, &state.dir));
    }
    let search_id = editor::new_copy(app, search_id);
    let list_id = editor::new_copy(app, list_id);
    let focus = app.pages.focus[page_id];
    insert(
        app,
        PageContent::Launcher(state),
        vec![search_id, list_id],
        focus,
    )
}

pub(super) fn teardown(page_id: PageId, app: &mut App, io: &mut dyn IO) {
    if let Some(loading) = &state(app, page_id).loading {
        io.process_kill(loading.process_id);
    }
}

pub(super) fn tick(page_id: PageId, app: &mut App, io: &mut dyn IO) {
    let LauncherEditors { search_id, list_id } = editors(app, page_id);

    poll_commands(page_id, app, io);
    search_id.tick(app, io);
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

fn editors(app: &App, page_id: PageId) -> LauncherEditors {
    let &[search_id, list_id] = app.pages.editor_ids[page_id].as_slice() else {
        unreachable!()
    };
    LauncherEditors { search_id, list_id }
}

fn state(app: &App, page_id: PageId) -> &State {
    let PageContent::Launcher(state) = &app.pages.content[page_id] else {
        unreachable!()
    };
    state
}

fn state_mut(app: &mut App, page_id: PageId) -> &mut State {
    let PageContent::Launcher(state) = &mut app.pages.content[page_id] else {
        unreachable!()
    };
    state
}

fn submit(page_id: PageId, app: &mut App, io: &mut dyn IO, window_id: WindowId) {
    // Refresh the whole list rather than just the matches: the cursor line
    // is a position in the list buffer, so recomputing the matches alone
    // would launch whatever the new matches happen to hold at the line the
    // previous frame's list is showing.
    refresh_list(page_id, app);
    let list_id = editors(app, page_id).list_id;
    let Some(command) = selected_command(app, page_id, list_id) else {
        return;
    };
    let dir = state(app, page_id).dir.clone();
    io.process_spawn_detached(&dir, BStr::new(LAUNCH_COMMAND), &[command.as_bstr()]);
    window_id.close(app, io);
}

// Bring `matches` and the list buffer in line with the current search text.
// Both change together, so the list buffer's line numbers are always
// positions in `matches`.
fn refresh_list(page_id: PageId, app: &mut App) {
    let LauncherEditors { search_id, list_id } = editors(app, page_id);
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
        .commands
        .iter()
        .filter(|command| {
            pattern.is_empty()
                || command
                    .windows(pattern.len())
                    .any(|window| window == pattern.as_slice())
        })
        .cloned()
        .collect();
    state.list_text = bstr::join("\n", state.matches.iter().cloned()).into();
}

fn selected_command(app: &App, page_id: PageId, list_id: EditorId) -> Option<BString> {
    let list_buffer_id = app.editors.buffer_id[list_id];
    let offset = app.editors.cursors[list_id].last().unwrap().head.offset;
    let line = list_buffer_id.grid_from_offset(app, offset)[1];
    state(app, page_id).matches.get(line).cloned()
}

// `complete -C` reads the interactive command line. Supplying the empty
// line explicitly produces the same completions from a non-interactive
// fish, which is what Focus's process API starts.
fn spawn_load(io: &mut dyn IO, dir: &Path) -> Loading {
    Loading {
        process_id: io.process_spawn(dir, BStr::new(COMPLETE_COMMAND), &[]),
        output: Vec::new(),
    }
}

// Drain the completion process. Fish takes tens of milliseconds to start,
// and longer with a heavy config, so the page opens with an empty list and
// fills it in whenever the output arrives. A fish that never exits leaves
// the launcher empty rather than hanging the app.
fn poll_commands(page_id: PageId, app: &mut App, io: &mut dyn IO) {
    let Some(loading) = &state(app, page_id).loading else {
        return;
    };
    let poll = io.process_poll(loading.process_id);

    let state = state_mut(app, page_id);
    let loading = state.loading.as_mut().unwrap();
    loading.output.extend_from_slice(&poll.new_output);
    if poll.exit_code.is_none() {
        return;
    }

    let output = take(&mut loading.output);
    state.loading = None;
    state.commands = parse_commands(&output);
    // The commands changed, so the matches are stale even though the
    // search text has not changed.
    state.last_pattern = None;
}

// Each completion line is "completion\tdescription". Fish describes an
// executable as exactly "command", or "command link" for a symlink to one;
// everything else - functions, files, and builtins whose description merely
// mentions the word, like `and`, `end` and `time` - is not launchable.
fn parse_commands(output: &[u8]) -> Vec<BString> {
    output
        .lines()
        .filter_map(|line| {
            let first_tab = line.find_byte(b'\t')?;
            let last_tab = line.rfind_byte(b'\t')?;
            let completion = &line[..first_tab];
            let completion_type = &line[last_tab + 1..];
            (!completion.is_empty() && matches!(completion_type, b"command" | b"command link"))
                .then(|| completion.into())
        })
        .collect()
}
