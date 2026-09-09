use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use bstr::{BStr, BString, ByteSlice};

use crate::{
    app::{App, IO},
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
    commands: Vec<BString>,
    matches: Vec<BString>,
    list_text: BString,
    last_pattern: Option<BString>,
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
    let dir = io.current_dir();
    let commands = load_commands(io, &dir);
    let search_id = editor::new_scratch(app);
    let list_id = editor::new_generated(app);

    insert(
        app,
        PageContent::Launcher(State {
            dir,
            commands,
            matches: Vec::new(),
            list_text: BString::default(),
            last_pattern: None,
        }),
        vec![search_id, list_id],
        SEARCH_IX,
    )
}

pub(super) fn duplicate(page_id: PageId, app: &mut App, _io: &mut dyn IO) -> PageId {
    let LauncherEditors { search_id, list_id } = editors(app, page_id);
    let state = state(app, page_id).clone();
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

pub(super) fn tick(page_id: PageId, app: &mut App, io: &mut dyn IO) {
    let LauncherEditors { search_id, list_id } = editors(app, page_id);

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
        list_id.cursor_reset(app);
    }

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
    let LauncherEditors { search_id, list_id } = editors(app, page_id);
    refresh_matches(app, page_id, search_id);
    let Some(command) = selected_command(app, page_id, list_id) else {
        return;
    };
    let dir = state(app, page_id).dir.clone();
    io.process_spawn_detached(&dir, BStr::new(LAUNCH_COMMAND), &[command.as_bstr()]);
    window_id.close(app, io);
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

fn load_commands(io: &mut dyn IO, dir: &Path) -> Vec<BString> {
    // `complete -C` reads the interactive command line. Supplying the empty
    // line explicitly produces the same completions from a non-interactive
    // fish, which is what Focus's process API starts.
    let output = process_output(io, dir, BStr::new(COMPLETE_COMMAND), &[]);
    output
        .lines()
        .filter_map(|line| {
            let first_tab = line.find_byte(b'\t')?;
            let last_tab = line.rfind_byte(b'\t')?;
            let completion = &line[..first_tab];
            let completion_type = &line[last_tab + 1..];
            (!completion.is_empty()
                && completion_type
                    .windows(b"command".len())
                    .any(|window| window == b"command"))
            .then(|| completion.into())
        })
        .collect()
}

fn process_output(io: &mut dyn IO, dir: &Path, command: &BStr, args: &[&BStr]) -> Vec<u8> {
    let process_id = io.process_spawn(dir, command, args);
    let mut output = Vec::new();
    loop {
        let poll = io.process_poll(process_id);
        output.extend_from_slice(&poll.new_output);
        if poll.exit_code.is_some() {
            return output;
        }
        thread::sleep(Duration::from_millis(1));
    }
}
