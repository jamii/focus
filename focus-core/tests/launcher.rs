use std::path::PathBuf;

use focus_core::app::App;
use focus_core::fuzz::MockIO;
use focus_core::input::{InputEvent, Key, NamedKey};
use focus_core::window::{self, WindowId};

mod common;

const SEARCH: usize = 0;
const LIST: usize = 1;

fn buffer_text(app: &App, n: usize) -> String {
    app.buffers.keys().nth(n).unwrap().text(app).to_string()
}

// The completion process is drained during tick, so a launcher is only
// loaded once one has happened.
fn launcher_app(completions: &[u8]) -> (App, MockIO, WindowId) {
    let mut io = MockIO::new();
    io.next_process_output = completions.to_vec();
    io.next_process_exit_code = Some(0);
    let mut app = App::new(&mut io);
    let window_id = window::open_launcher(&mut app, &mut io);
    common::tick(&mut app, &mut io);
    (app, io, window_id)
}

// A launcher whose completion process has not exited yet.
fn loading_launcher_app(completions: &[u8]) -> (App, MockIO, WindowId) {
    let mut io = MockIO::new();
    io.next_process_output = completions.to_vec();
    io.next_process_exit_code = None;
    let mut app = App::new(&mut io);
    let window_id = window::open_launcher(&mut app, &mut io);
    common::tick(&mut app, &mut io);
    (app, io, window_id)
}

#[test]
fn loads_only_command_completions_and_shows_only_the_completion_text() {
    // Fish describes an executable as exactly "command", or "command link"
    // for a symlink. Builtins carry a prose description, which for several of
    // them (`and`, `end`, `time`, ...) contains the word "command".
    let (app, io, _window_id) = launcher_app(
        b"alias\tfunction\n\
          and\tRun command if last command succeeded\n\
          bat\tcommand\n\
          cc\tcommand link\n\
          commandline\tSet or get the commandline\n\
          src/\tdirectory\n",
    );

    assert_eq!(io.processes.len(), 1);
    assert_eq!(io.processes[0].dir, PathBuf::from("/"));
    assert_eq!(io.processes[0].command, "complete -C ''");
    assert!(io.processes[0].args.is_empty());
    assert_eq!(buffer_text(&app, SEARCH), "");
    assert_eq!(buffer_text(&app, LIST), "bat\ncc");
    app.assert_invariants();
}

#[test]
fn filtering_is_case_sensitive_exact_substring_matching() {
    let (mut app, mut io, window_id) =
        launcher_app(b"Alpha\tcommand\nalpine\tcommand\ncaliper\tcommand\n");

    common::text_input(&mut app, &mut io, window_id, "Al");
    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, LIST), "Alpha");

    common::control_key(&mut app, &mut io, window_id, Key::Character("z"));
    common::text_input(&mut app, &mut io, window_id, "al");
    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, LIST), "alpine\ncaliper");
    app.assert_invariants();
}

#[test]
fn ctrl_ik_change_the_selected_command() {
    let (mut app, mut io, window_id) =
        launcher_app(b"alpha\tcommand\nbeta\tcommand\ngamma\tcommand\n");

    common::control_key(&mut app, &mut io, window_id, Key::Character("k"));
    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));

    assert_eq!(io.detached.len(), 1);
    assert_eq!(io.detached[0].args, ["beta"]);
    app.assert_invariants();
}

#[test]
fn ctrl_enter_uses_the_current_search_launches_disowned_and_closes() {
    let (mut app, mut io, window_id) =
        launcher_app(b"cargo\tcommand\nfoot\tcommand\nfootclient\tcommand\n");
    common::text_input(&mut app, &mut io, window_id, "footc");

    // Submit before a tick to ensure the current input, rather than stale
    // matches from the previous frame, chooses the command.
    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));

    assert_eq!(io.detached.len(), 1);
    assert_eq!(io.detached[0].dir, PathBuf::from("/"));
    assert_eq!(io.detached[0].command, "eval \"$argv[1] &\"; disown");
    assert_eq!(io.detached[0].args, ["footclient"]);
    assert!(io.open_windows.is_empty());
    assert!(!io.exited);
    app.assert_invariants();
}

#[test]
fn launching_closes_only_the_launcher_window() {
    let (mut app, mut io, launcher_window_id) = launcher_app(b"cargo\tcommand\n");
    let scratch_window_id = window::open_scratch(&mut app, &mut io);
    common::tick(&mut app, &mut io);

    common::control_key(
        &mut app,
        &mut io,
        launcher_window_id,
        Key::Named(NamedKey::Enter),
    );

    assert_eq!(io.open_windows, [scratch_window_id]);
    assert!(!io.exited);
    app.assert_invariants();
}

#[test]
fn ctrl_enter_with_no_match_does_not_launch_or_close() {
    let (mut app, mut io, window_id) = launcher_app(b"cargo\tcommand\n");
    common::text_input(&mut app, &mut io, window_id, "missing");

    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));

    assert!(io.detached.is_empty());
    assert_eq!(io.open_windows, [window_id]);
    assert!(!io.exited);
    app.assert_invariants();
}

#[test]
fn duplicating_the_launcher_copies_its_loaded_picker() {
    let (mut app, mut io, window_id) =
        launcher_app(b"cargo\tcommand\nfoot\tcommand\nfootclient\tcommand\n");
    common::text_input(&mut app, &mut io, window_id, "foot");
    common::tick(&mut app, &mut io);

    common::control_key(&mut app, &mut io, window_id, Key::Character("n"));
    let copy_window_id = *io.open_windows.last().unwrap();
    common::tick(&mut app, &mut io);

    assert_eq!(io.processes.len(), 1);
    assert_eq!(buffer_text(&app, SEARCH), "foot");
    assert_eq!(buffer_text(&app, LIST), "foot\nfootclient");
    assert_eq!(buffer_text(&app, 2), "foot");
    assert_eq!(buffer_text(&app, 3), "foot\nfootclient");

    common::char_input(&mut app, &mut io, copy_window_id, 'c');
    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, SEARCH), "foot");
    assert_eq!(buffer_text(&app, 2), "footc");
    app.assert_invariants();
}

#[test]
fn the_window_opens_before_the_completions_arrive() {
    // Loading must not block: the launcher is usable, with an empty list,
    // until the completion process exits.
    let (mut app, mut io, window_id) = loading_launcher_app(b"cargo\tcommand\nfoo");
    assert_eq!(io.open_windows, [window_id]);
    assert_eq!(buffer_text(&app, LIST), "");

    common::text_input(&mut app, &mut io, window_id, "car");
    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, SEARCH), "car");
    assert_eq!(buffer_text(&app, LIST), "");

    // The rest of the output arrives, then the process exits.
    io.processes[0].pending_output = b"t\tcommand\ncarp\tcommand\n".to_vec();
    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, LIST), "");
    io.processes[0].exit_code = Some(0);
    common::tick(&mut app, &mut io);

    assert_eq!(buffer_text(&app, LIST), "cargo\ncarp");
    app.assert_invariants();
}

#[test]
fn editing_the_search_relists_before_launching() {
    // The cursor line is a position in the list buffer, so an edit made
    // after selecting a line must relist before the line is read - what
    // launches is what the search text currently matches.
    let (mut app, mut io, window_id) =
        launcher_app(b"book\tcommand\nfoot\tcommand\nfootclient\tcommand\n");
    common::text_input(&mut app, &mut io, window_id, "oot");
    common::tick(&mut app, &mut io);
    common::control_key(&mut app, &mut io, window_id, Key::Character("k"));
    assert_eq!(buffer_text(&app, LIST), "foot\nfootclient");

    common::key(
        &mut app,
        &mut io,
        window_id,
        Key::Named(NamedKey::Backspace),
    );
    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));

    assert_eq!(io.detached.len(), 1);
    assert_eq!(io.detached[0].args, ["book"]);
    app.assert_invariants();
}

#[test]
fn duplicating_a_loading_launcher_runs_its_own_completion_process() {
    // A process can only be drained by one page.
    let (mut app, mut io, window_id) = loading_launcher_app(b"cargo\tcommand\n");

    common::control_key(&mut app, &mut io, window_id, Key::Character("n"));
    common::tick(&mut app, &mut io);

    assert_eq!(io.processes.len(), 2);
    assert_eq!(io.processes[1].command, "complete -C ''");
    app.assert_invariants();
}

#[test]
fn closing_a_loading_launcher_kills_the_completion_process() {
    let (mut app, mut io, window_id) = loading_launcher_app(b"cargo\tcommand\n");

    app.input(&mut io, window_id, InputEvent::CloseRequested);

    assert!(io.processes[0].killed);
    assert!(io.open_windows.is_empty());
    app.assert_invariants();
}
