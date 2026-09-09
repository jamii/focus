use std::path::PathBuf;

use focus_core::app::App;
use focus_core::fuzz::MockIO;
use focus_core::input::{Key, NamedKey};
use focus_core::window::{self, WindowId};

mod common;

const SEARCH: usize = 0;
const LIST: usize = 1;

fn buffer_text(app: &App, n: usize) -> String {
    app.buffers.keys().nth(n).unwrap().text(app).to_string()
}

fn launcher_app(completions: &[u8]) -> (App, MockIO, WindowId) {
    let mut io = MockIO::new();
    io.next_process_output = completions.to_vec();
    io.next_process_exit_code = Some(0);
    let mut app = App::new(&mut io);
    let window_id = window::open_launcher(&mut app, &mut io);
    (app, io, window_id)
}

#[test]
fn loads_only_command_completions_and_shows_only_the_completion_text() {
    let (mut app, mut io, _window_id) = launcher_app(
        b"alias\tfunction\n\
          bat\tcommand\n\
          cc\tcommand link\n\
          src/\tdirectory\n\
          tool\tinstalled executable\twrapped command\n",
    );
    common::tick(&mut app, &mut io);

    assert_eq!(io.processes.len(), 1);
    assert_eq!(io.processes[0].dir, PathBuf::from("/"));
    assert_eq!(io.processes[0].command, "complete -C ''");
    assert!(io.processes[0].args.is_empty());
    assert_eq!(buffer_text(&app, SEARCH), "");
    assert_eq!(buffer_text(&app, LIST), "bat\ncc\ntool");
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
    common::tick(&mut app, &mut io);

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
