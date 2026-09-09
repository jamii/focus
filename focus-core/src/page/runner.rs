use std::ops::Range;
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use bstr::{BStr, BString, ByteSlice};

use crate::{
    app::{App, IO, ProcessId},
    buffer::{self, OffsetDiff},
    drawing::Rect,
    editor::{self, EditorId},
    input::{ButtonState, InputEvent, Key, NamedKey},
    window::WindowId,
};

use super::{GAP, PageContent, PageId, insert, new_edit};

#[derive(Clone)]
pub(super) struct State {
    dir: PathBuf,
    command: BString,
    process_id: ProcessId,
    // app.save_count when the process was (re)spawned.
    spawn_save_count: u64,
    // app.frame_start when the process was (re)spawned.
    started_at: Duration,
    exited: bool,
    // Output before this offset has been parsed for locations. Always the
    // start of a line, so a location split across two polls parses whole.
    // Kept in step with edits to the output buffer by handle_edits.
    parsed_offset: usize,
    locations: Vec<Location>,
}

// A `path:line[:col]` location parsed from the command's output.
#[derive(Clone)]
struct Location {
    // Byte range of the `path:line[:col]` token in the output buffer. Kept
    // in step with edits to the output buffer by handle_edits.
    report_range: Range<usize>,
    path: PathBuf,
    // 1-based, as reported by compilers.
    line: usize,
    col: usize,
}

pub(super) const EDITOR_COUNT: usize = 2;

// How much command output to keep. A command can produce output without
// end, so the oldest is dropped rather than growing the buffer until the
// editor runs out of memory. Trimming only when twice the limit has built
// up keeps it to one copy per MAX_OUTPUT_BYTES of output.
const MAX_OUTPUT_BYTES: usize = 1024 * 1024;

const OUTPUT_IX: usize = 0;

pub(crate) fn new(app: &mut App, io: &mut dyn IO, dir: PathBuf, command: BString) -> PageId {
    // Parsed locations are resolved against dir, and file buffers require
    // absolute paths.
    assert!(dir.is_absolute(), "runner dir must be absolute: {:?}", dir);
    let output_id = editor::new_generated(app);
    let status_id = editor::new_generated(app);
    let process_id = io.process_spawn(&dir, command.as_bstr(), &[]);
    let spawn_save_count = app.save_count;
    let started_at = app.frame_start;
    insert(
        app,
        PageContent::Runner(State {
            dir,
            command,
            process_id,
            spawn_save_count,
            started_at,
            exited: false,
            parsed_offset: 0,
            locations: Vec::new(),
        }),
        vec![output_id, status_id],
        OUTPUT_IX,
    )
}

pub(super) fn duplicate(page_id: PageId, app: &mut App, io: &mut dyn IO) -> PageId {
    // A process can only be drained by one page - process_poll hands each
    // chunk of output to a single caller, and teardown kills it - so the
    // copy runs the command again in its own process rather than sharing.
    let (dir, command, focus) = {
        let state = state(app, page_id);
        (
            state.dir.clone(),
            state.command.clone(),
            app.pages.focus[page_id],
        )
    };
    let copy_id = new(app, io, dir, command);
    app.pages.focus[copy_id] = focus;
    copy_id
}

// Format an elapsed duration at the coarsest useful resolution: "42s",
// "3m42s", "1h03m".
fn format_elapsed(elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    let (h, m, s) = (secs / 3600, (secs / 60) % 60, secs % 60);
    if h > 0 {
        format!("{}h{:02}m", h, m)
    } else if m > 0 {
        format!("{}m{:02}s", m, s)
    } else {
        format!("{}s", s)
    }
}

// The part of tick that must run whether or not the page is visible:
// restart on save, drain the process, and parse its output. Draining every
// frame keeps the backend's output buffer small while an edit page is
// pushed on top; restarting promptly means the output is fresh on return.
pub(super) fn tick_background(page_id: PageId, app: &mut App, io: &mut dyn IO) {
    // Restart the command when any file has been saved since it started.
    if app.save_count > state(app, page_id).spawn_save_count {
        restart(page_id, app, io);
    }

    let output_id = editors(app, page_id).output_id;
    let output_buffer_id = app.editors.buffer_id[output_id];

    // Append any new output at the buffer end. A cursor sitting at the
    // buffer end is carried forward by the append (so it tails the
    // output); cursors elsewhere stay put. Generated output is not
    // undoable, so it doesn't pile up on the undo stack.
    let poll = io.process_poll(state(app, page_id).process_id);
    output_buffer_id.append(app, poll.new_output.as_bstr());
    if let Some(exit_code) = poll.exit_code
        && !state(app, page_id).exited
    {
        state_mut(app, page_id).exited = true;
        let exit_text = format!("\n[exited: {}]\n", exit_code);
        output_buffer_id.append(app, BStr::new(exit_text.as_bytes()));
    }

    // Drop the oldest output once it has built up. This is not an append,
    // so handle_edits remaps the parse position and shifts every location,
    // dropping the ones that were in the removed text.
    if output_buffer_id.text(app).len() > 2 * MAX_OUTPUT_BYTES {
        output_buffer_id.trim_front_to(app, MAX_OUTPUT_BYTES);
    }

    // Parse any newly completed lines for locations.
    let dir = state(app, page_id).dir.clone();
    let parsed_offset = state(app, page_id).parsed_offset;
    let text = output_buffer_id.text(app);
    if let Some(last_newline) = text[parsed_offset..].rfind_byte(b'\n') {
        let parse_end = parsed_offset + last_newline + 1;
        let locations = parse_locations(&text[parsed_offset..parse_end], parsed_offset, &dir);
        let state = state_mut(app, page_id);
        state.locations.extend(locations);
        state.parsed_offset = parse_end;
    }
}

pub(super) fn tick(page_id: PageId, app: &mut App, io: &mut dyn IO) {
    tick_background(page_id, app, io);

    let RunnerEditors {
        output_id,
        status_id,
    } = editors(app, page_id);

    // Update the status bar: command and time since it last started.
    let status_buffer_id = app.editors.buffer_id[status_id];
    let (command, started_at) = {
        let s = state(app, page_id);
        (s.command.clone(), s.started_at)
    };
    let elapsed = app.frame_start.saturating_sub(started_at);
    let status_text = format!("{} (started {} ago)", command, format_elapsed(elapsed));
    status_buffer_id.replace(app, BStr::new(status_text.as_bytes()));

    status_id.tick(app, io);
    output_id.tick(app, io);
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
        logical_key,
    } = event
        && app.modifiers.control
        && !app.modifiers.alt
    {
        // Ctrl+enter opens the location under the cursor.
        if let Key::Named(NamedKey::Enter) = logical_key {
            submit_selected(page_id, app, io, window_id);
            return true;
        }
        // Ctrl+r restarts the command.
        if let Key::Character("r") = logical_key {
            restart(page_id, app, io);
            return true;
        }
    }

    false
}

pub(super) fn layout(page_rect: Rect, cell_size: [u32; 2]) -> Vec<Rect> {
    let [output_rect, status_rect] = page_rect.split_from_bottom(cell_size[1] as f32, GAP);
    vec![output_rect, status_rect]
}

// Keep the parse position and location ranges in step with edits to the
// output buffer, whether from the user or from appended output. Appends
// at the end leave every earlier offset alone, so this is cheap in the
// common case.
pub(super) fn handle_edits(page_id: PageId, app: &mut App, editor_ix: usize, diff: &OffsetDiff) {
    if editor_ix != OUTPUT_IX {
        return;
    }
    // Every location sits below the parse position, so if that is below the
    // first edit then nothing tracked here moved. Appended output always
    // is, which keeps streaming independent of how many locations have
    // piled up.
    if state(app, page_id).parsed_offset <= diff.unchanged_before() {
        return;
    }
    let state = state_mut(app, page_id);
    // An insert at exactly parsed_offset is unparsed text, so the parse
    // position stays before it.
    state.parsed_offset = diff.apply_before(state.parsed_offset);
    state.locations.retain_mut(
        |location| match diff.apply_range(location.report_range.clone()) {
            Some(range) => {
                location.report_range = range;
                true
            }
            None => false,
        },
    );
}

pub(super) fn current_path(_page_id: PageId, _app: &App) -> Option<PathBuf> {
    None
}

// Kill the command when the page is removed from its window's stack, so
// it doesn't keep running invisibly.
pub(super) fn teardown(page_id: PageId, app: &mut App, io: &mut dyn IO) {
    let process_id = state(app, page_id).process_id;
    io.process_kill(process_id);
}

#[derive(Clone, Copy)]
struct RunnerEditors {
    output_id: EditorId,
    status_id: EditorId,
}

fn editors(app: &App, page_id: PageId) -> RunnerEditors {
    let &[output_id, status_id] = app.pages.editor_ids[page_id].as_slice() else {
        unreachable!()
    };
    RunnerEditors {
        output_id,
        status_id,
    }
}

fn state(app: &App, page_id: PageId) -> &State {
    let PageContent::Runner(state) = &app.pages.content[page_id] else {
        unreachable!()
    };
    state
}

fn state_mut(app: &mut App, page_id: PageId) -> &mut State {
    let PageContent::Runner(state) = &mut app.pages.content[page_id] else {
        unreachable!()
    };
    state
}

// Kill the process and spawn it afresh with an empty output buffer.
fn restart(page_id: PageId, app: &mut App, io: &mut dyn IO) {
    let output_id = editors(app, page_id).output_id;
    let output_buffer_id = app.editors.buffer_id[output_id];
    let (old_process_id, dir, command) = {
        let state = state(app, page_id);
        (state.process_id, state.dir.clone(), state.command.clone())
    };
    io.process_kill(old_process_id);
    // The old output is gone for good: no undo history to keep.
    output_buffer_id.replace(app, BStr::new(b""));
    let process_id = io.process_spawn(&dir, command.as_bstr(), &[]);
    let spawn_save_count = app.save_count;
    let started_at = app.frame_start;
    let state = state_mut(app, page_id);
    state.process_id = process_id;
    state.spawn_save_count = spawn_save_count;
    state.started_at = started_at;
    state.exited = false;
    state.parsed_offset = 0;
    state.locations.clear();
}

// Open an edit page for the location under the main cursor: the location
// whose report range contains the cursor, or failing that the first
// location starting on the cursor's line.
fn submit_selected(page_id: PageId, app: &mut App, io: &mut dyn IO, window_id: WindowId) {
    let output_id = editors(app, page_id).output_id;
    let output_buffer_id = app.editors.buffer_id[output_id];
    let offset = output_id.main_cursor_offset(app);
    let line = output_buffer_id.grid_from_offset(app, offset)[1];
    let location = {
        let locations = &state(app, page_id).locations;
        locations
            .iter()
            .find(|location| {
                location.report_range.start <= offset && offset <= location.report_range.end
            })
            .or_else(|| {
                locations.iter().find(|location| {
                    output_buffer_id.grid_from_offset(app, location.report_range.start)[1] == line
                })
            })
            .cloned()
    };
    let Some(location) = location else {
        return;
    };

    let buffer_id = buffer::from_file(app, location.path);
    // Load the file now so the target offset can be computed.
    buffer_id.tick(app, io);
    let target = buffer_id.offset_from_grid(
        app,
        [
            location.col.saturating_sub(1),
            location.line.saturating_sub(1),
        ],
    );
    let editor_id = editor::new(app, buffer_id);
    editor_id.set_cursor_offsets(app, &[target]);
    editor_id.scroll_offset_into_center(app, target);
    let page_id = new_edit(app, editor_id);
    // Push rather than replace, so ctrl+q returns to the output.
    window_id.push_page(app, io, page_id);
}

// Scan for `path:line[:col]` tokens: a path is a run of bytes that are
// neither whitespace nor ':', line/col are runs of digits, and col
// defaults to 1 (so `file:42: message` from gcc, grep -n and make works
// as well as `file:42:7`). Offsets in the returned ranges are relative to
// the output buffer (`text` starts at `base` within it).
fn parse_locations(text: &BStr, base: usize, dir: &Path) -> Vec<Location> {
    let mut locations = Vec::new();
    let mut i = 0;
    while i < text.len() {
        let path_start = i;
        while i < text.len() && !matches!(text[i], b' ' | b'\t' | b'\r' | b'\n' | b':') {
            i += 1;
        }
        let path_end = i;
        if path_start == path_end {
            i += 1;
            continue;
        }

        if i < text.len() && text[i] == b':' {
            i += 1;
        } else {
            continue;
        }

        let line_start = i;
        while i < text.len() && text[i].is_ascii_digit() {
            i += 1;
        }
        let line_end = i;
        if line_start == line_end {
            continue;
        }

        // Only take a column if digits follow the ':', so a trailing ':'
        // before the message is left alone and the location still counts.
        let mut col_range = None;
        if i + 1 < text.len() && text[i] == b':' && text[i + 1].is_ascii_digit() {
            i += 1;
            let col_start = i;
            while i < text.len() && text[i].is_ascii_digit() {
                i += 1;
            }
            col_range = Some(col_start..i);
        }

        let Some(line) = parse_usize(&text[line_start..line_end]) else {
            continue;
        };
        let col = match col_range {
            None => 1,
            Some(range) => match parse_usize(&text[range]) {
                Some(col) => col,
                None => continue,
            },
        };

        let path = Path::new(std::ffi::OsStr::from_bytes(&text[path_start..path_end]));
        locations.push(Location {
            report_range: base + path_start..base + i,
            path: normalize(dir.join(path)),
            line,
            col,
        });
    }
    locations
}

fn parse_usize(digits: &[u8]) -> Option<usize> {
    // Digits are ascii, so from_utf8 cannot fail; parse fails on overflow.
    std::str::from_utf8(digits).ok()?.parse().ok()
}

// Resolve `.` and `..` lexically, without touching the filesystem.
fn normalize(path: PathBuf) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            component => out.push(component),
        }
    }
    out
}
