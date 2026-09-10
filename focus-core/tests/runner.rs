use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use focus_core::app::App;
use focus_core::buffer;
use focus_core::fuzz::MockIO;
use focus_core::input::{Key, NamedKey};
use focus_core::window::{self, WindowId};

mod common;

// Buffer creation order for the full ctrl+m flow starting from scratch_app():
//   scratch_app() creates: editor buffer (0), status bar buffer (1)
//   ctrl+m → dir picker: path_id (2), list_id (3)
//   alt+enter → command picker: preview_id (4), search_id (5), list_id (6)
//   run command → runner: output_id (7), status_id (8)
const DIR_PATH: usize = 2;
const DIR_LIST: usize = 3;
const SEARCH: usize = 5;
const LIST: usize = 6;
const OUTPUT: usize = 7;
const MAKER_STATUS: usize = 8;

const FISH_HISTORY_PATH: &str = "/.local/share/fish/fish_history";

fn buffer_text(app: &App, n: usize) -> String {
    app.buffers.keys().nth(n).unwrap().text(app).to_string()
}

fn insert_file(io: &mut MockIO, path: &str, text: &str) {
    io.files.insert(
        PathBuf::from(path),
        (
            text.as_bytes().to_vec(),
            SystemTime::UNIX_EPOCH + Duration::from_secs(1),
        ),
    );
}

// Assert that the nth one-shot process is fish recording `command` in its
// history. The text is passed as an argument, so it reaches fish verbatim.
fn assert_history_append(io: &MockIO, n: usize, command: &str) {
    let detached = &io.detached[n];
    assert_eq!(detached.command, "history append -- $argv[1]");
    assert_eq!(detached.args.len(), 1);
    assert_eq!(detached.args[0], command);
}

// Open the dir picker (ctrl+m), accept the default dir (alt+enter), and
// land on the command picker. The fish history file is pre-populated.
fn choose_command_app(history: &str) -> (App, MockIO, WindowId) {
    let (mut app, mut io, window_id) = common::scratch_app();
    insert_file(&mut io, FISH_HISTORY_PATH, history);
    // ctrl+m → dir picker
    common::control_key(&mut app, &mut io, window_id, Key::Character("m"));
    // alt+enter → accept current dir, enter command picker
    common::alt_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    (app, io, window_id)
}

// Run `command` by typing it into the command picker and pressing alt+enter.
fn runner_app(command: &str) -> (App, MockIO, WindowId) {
    let (mut app, mut io, window_id) = choose_command_app("");
    common::text_input(&mut app, &mut io, window_id, command);
    common::alt_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    (app, io, window_id)
}

// ── dir picker ────────────────────────────────────────────────────────────────

#[test]
fn dir_picker_shows_subdirectories() {
    let (mut app, mut io, window_id) = common::scratch_app();
    // Insert some files so dirs appear.
    insert_file(&mut io, "/proj/src/main.rs", "");
    insert_file(&mut io, "/proj/tests/foo.rs", "");
    insert_file(&mut io, "/other/bar.rs", "");

    common::control_key(&mut app, &mut io, window_id, Key::Character("m"));
    common::tick(&mut app, &mut io);

    // The list should show only subdirectories of `/` (other/, proj/).
    let list = buffer_text(&app, DIR_LIST);
    assert!(list.contains("proj/"), "expected proj/ in list: {list:?}");
    assert!(list.contains("other/"), "expected other/ in list: {list:?}");
    // No files should appear.
    assert!(!list.contains("main.rs"), "no files expected: {list:?}");
    app.assert_invariants();
}

#[test]
fn dir_picker_ctrl_enter_descends_into_selected_dir() {
    let (mut app, mut io, window_id) = common::scratch_app();
    insert_file(&mut io, "/proj/src/main.rs", "");

    common::control_key(&mut app, &mut io, window_id, Key::Character("m"));
    common::tick(&mut app, &mut io);

    // ctrl+enter descends into the first (and only) listed dir (proj/).
    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::tick(&mut app, &mut io);

    // The path editor now shows /proj/.
    assert_eq!(buffer_text(&app, DIR_PATH), "/proj/");
    // The list now shows src/.
    assert!(buffer_text(&app, DIR_LIST).contains("src/"));
    app.assert_invariants();
}

#[test]
fn dir_picker_ctrl_enter_with_a_filter_drops_the_filter() {
    let (mut app, mut io, window_id) = common::scratch_app();
    insert_file(&mut io, "/proj/src/main.rs", "");
    insert_file(&mut io, "/proj/tests/foo.rs", "");

    common::control_key(&mut app, &mut io, window_id, Key::Character("m"));
    common::tick(&mut app, &mut io);
    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, DIR_PATH), "/proj/");
    assert_eq!(buffer_text(&app, DIR_LIST), "src/\ntests/");

    // Typing a filter narrows the list; descending uses the listed dir
    // plus the entry, not the path text with the filter still on it.
    common::text_input(&mut app, &mut io, window_id, "s");
    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, DIR_PATH), "/proj/s");
    assert_eq!(buffer_text(&app, DIR_LIST), "src/\ntests/");
    common::text_input(&mut app, &mut io, window_id, "r");
    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, DIR_LIST), "src/");
    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, DIR_PATH), "/proj/src/");
    app.assert_invariants();
}

#[test]
fn dir_picker_alt_enter_rejects_a_relative_dir() {
    let (mut app, mut io, window_id) = common::scratch_app();
    insert_file(&mut io, "/proj/src/main.rs", "");

    common::control_key(&mut app, &mut io, window_id, Key::Character("m"));
    // Replace the leading `/` with a relative path.
    common::key(
        &mut app,
        &mut io,
        window_id,
        Key::Named(NamedKey::Backspace),
    );
    common::text_input(&mut app, &mut io, window_id, "proj/");
    assert_eq!(buffer_text(&app, DIR_PATH), "proj/");
    common::alt_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::tick(&mut app, &mut io);

    // Still on the dir picker: no command picker buffers, no process.
    assert_eq!(app.buffers.keys().count(), DIR_LIST + 1);
    assert!(io.processes.is_empty());
    common::char_input(&mut app, &mut io, window_id, 'x');
    assert_eq!(buffer_text(&app, DIR_PATH), "proj/x");
    app.assert_invariants();
}

#[test]
fn dir_picker_alt_enter_rejects_a_dir_that_does_not_exist() {
    // Typing `/foo` used to choose `/`, because everything after the last
    // `/` counted as a filter, so the command quietly ran somewhere the
    // user had not picked. `/foo/` used to be accepted and only failed
    // later, at spawn time.
    for typed in ["foo", "foo/"] {
        let (mut app, mut io, window_id) = common::scratch_app();
        insert_file(&mut io, "/proj/src/main.rs", "");

        common::control_key(&mut app, &mut io, window_id, Key::Character("m"));
        common::text_input(&mut app, &mut io, window_id, typed);
        common::tick(&mut app, &mut io);

        common::alt_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
        common::tick(&mut app, &mut io);

        // Still on the dir picker: nothing spawned, no command picker.
        assert!(io.processes.is_empty(), "spawned for {:?}", typed);
        assert_eq!(app.buffers.keys().count(), DIR_LIST + 1);
        common::char_input(&mut app, &mut io, window_id, 'x');
        assert_eq!(buffer_text(&app, DIR_PATH), format!("/{}x", typed));
        app.assert_invariants();
    }
}

#[test]
fn dir_picker_alt_enter_accepts_a_dir_typed_without_a_trailing_slash() {
    let (mut app, mut io, window_id) = common::scratch_app();
    insert_file(&mut io, "/proj/src/main.rs", "");

    common::control_key(&mut app, &mut io, window_id, Key::Character("m"));
    common::text_input(&mut app, &mut io, window_id, "proj");
    common::alt_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::text_input(&mut app, &mut io, window_id, "ls");
    common::alt_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));

    assert_eq!(io.processes[0].dir, PathBuf::from("/proj"));
    app.assert_invariants();
}

#[test]
fn dir_picker_alt_enter_accepts_current_dir_and_opens_command_picker() {
    let (mut app, mut io, window_id) = common::scratch_app();
    insert_file(&mut io, FISH_HISTORY_PATH, "- cmd: make\n  when: 1\n");

    common::control_key(&mut app, &mut io, window_id, Key::Character("m"));
    // alt+enter while the path says "/" → command picker rooted at /.
    common::alt_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::tick(&mut app, &mut io);

    // We are now on the command picker; the list shows the history.
    assert_eq!(buffer_text(&app, LIST), "make");
    app.assert_invariants();
}

#[test]
fn dir_picker_alt_enter_accepts_dir_with_no_entries() {
    // A directory with nothing inside is still valid to choose.
    let (mut app, mut io, window_id) = common::scratch_app();
    // No files at all; `dir_list("/")` returns Ok(empty) in MockIO only for `/`.
    common::control_key(&mut app, &mut io, window_id, Key::Character("m"));
    // alt+enter → command picker for /.
    common::alt_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::tick(&mut app, &mut io);

    // We landed on the command picker (empty list, empty search).
    assert_eq!(buffer_text(&app, LIST), "");
    app.assert_invariants();
}

// ── command picker (filtering) ─────────────────────────────────────────────

#[test]
fn picker_lists_history_most_recent_first_and_deduped() {
    let (mut app, mut io, _window_id) = choose_command_app(
        "- cmd: cargo test\n  when: 100\n- cmd: cargo build\n  when: 101\n- cmd: cargo test\n  when: 102\n",
    );
    common::tick(&mut app, &mut io);

    assert_eq!(buffer_text(&app, LIST), "cargo test\ncargo build");
    app.assert_invariants();
}

#[test]
fn filtering_is_exact_substring_in_recency_order() {
    // History newest→oldest: "ls -la", "echo ls", "make"
    let history = "- cmd: make\n  when: 1\n- cmd: echo ls\n  when: 2\n- cmd: ls -la\n  when: 3\n";
    let (mut app, mut io, window_id) = choose_command_app(history);
    common::tick(&mut app, &mut io);
    // Unfiltered: most-recent first.
    assert_eq!(buffer_text(&app, LIST), "ls -la\necho ls\nmake");

    // Filtering by "ls" keeps "ls -la" and "echo ls" (both contain "ls"),
    // in recency order; "make" has no "ls" substring so it drops out.
    common::text_input(&mut app, &mut io, window_id, "ls");
    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, LIST), "ls -la\necho ls");
    app.assert_invariants();
}

#[test]
fn filtering_does_not_fuzzy_match_across_words() {
    // "make" does NOT match "ls" even though m…a…k…e contains no "ls".
    // More concretely: "l" followed by non-adjacent "s" should NOT match.
    // In the old fuzzy scorer, "ls" would match "list --size" as a subsequence;
    // the new exact-substring filter should only match if "ls" appears literally.
    let history = "- cmd: local setup\n  when: 1\n- cmd: ls\n  when: 2\n";
    let (mut app, mut io, window_id) = choose_command_app(history);
    common::text_input(&mut app, &mut io, window_id, "ls");
    common::tick(&mut app, &mut io);

    // Only "ls" should appear; "local setup" has no literal "ls" substring.
    assert_eq!(buffer_text(&app, LIST), "ls");
    app.assert_invariants();
}

#[test]
fn ctrl_enter_runs_the_selected_history_entry() {
    let (mut app, mut io, window_id) =
        choose_command_app("- cmd: cargo test\n  when: 100\n- cmd: cargo build\n  when: 101\n");
    common::text_input(&mut app, &mut io, window_id, "build");
    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, SEARCH), "build");
    assert_eq!(buffer_text(&app, LIST), "cargo build");

    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));

    // The command is spawned at the chosen dir (default: /).
    assert_eq!(io.processes.len(), 1);
    assert_eq!(io.processes[0].command, "cargo build");
    assert_eq!(io.processes[0].dir, PathBuf::from("/"));
    // The command is a script for the shell to run, not an argument to one.
    assert!(io.processes[0].args.is_empty());
    // ...and fish is asked to record it, in the same dir.
    assert_eq!(io.detached.len(), 1);
    assert_history_append(&io, 0, "cargo build");
    assert_eq!(io.detached[0].dir, PathBuf::from("/"));
    // Focus writes nothing to the history file itself.
    assert_eq!(
        io.files[&PathBuf::from(FISH_HISTORY_PATH)].0,
        b"- cmd: cargo test\n  when: 100\n- cmd: cargo build\n  when: 101\n"
    );
    app.assert_invariants();
}

#[test]
fn alt_enter_runs_the_raw_input_text() {
    let (mut app, mut io, window_id) = choose_command_app("- cmd: cargo test\n  when: 100\n");
    common::text_input(&mut app, &mut io, window_id, "make bespoke");

    common::alt_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));

    assert_eq!(io.processes.len(), 1);
    assert_eq!(io.processes[0].command, "make bespoke");
    assert_eq!(io.detached.len(), 1);
    assert_history_append(&io, 0, "make bespoke");
    app.assert_invariants();
}

#[test]
fn history_append_passes_awkward_command_text_through_unquoted() {
    // Nothing here is quoted or escaped by us. A leading dash would be read
    // as an option if the text were not passed as an argument, which is the
    // case the `--` in the script guards against.
    let (mut app, mut io, window_id) = choose_command_app("");
    common::text_input(&mut app, &mut io, window_id, "--flag 'q' $HOME");
    common::alt_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));

    assert_eq!(io.processes[0].command, "--flag 'q' $HOME");
    assert_history_append(&io, 0, "--flag 'q' $HOME");
    app.assert_invariants();
}

#[test]
fn history_lines_are_listed_exactly_as_fish_wrote_them() {
    // A lone backslash is not an escape fish emits, but if one reaches the
    // file it has to be shown as it appears there. Re-deriving the display
    // by escaping the unescaped command used to double it.
    let (mut app, mut io, window_id) = choose_command_app("- cmd: a\\qb\n  when: 1\n");
    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, LIST), "a\\qb");

    // The command keeps the backslash too.
    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    assert_eq!(io.processes[0].command, "a\\qb");
    app.assert_invariants();
}

#[test]
fn escaped_history_entries_are_listed_on_one_line_and_run_unescaped() {
    // Fish escapes newlines and backslashes in cmd text, so a multi-line
    // command is one history line. The picker shows that escaped form, but
    // runs and records the real command.
    let (mut app, mut io, window_id) = choose_command_app("- cmd: echo a\\\\b\\ncd\n  when: 1\n");
    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, LIST), "echo a\\\\b\\ncd");

    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));

    assert_eq!(io.processes.len(), 1);
    assert_eq!(io.processes[0].command, "echo a\\b\ncd");
    // Fish is handed the unescaped text and re-escapes it itself.
    assert_history_append(&io, 0, "echo a\\b\ncd");
    app.assert_invariants();
}

#[test]
fn typing_in_the_picker_stays_fast_with_a_large_history() {
    // `replace` swaps generated text outright. Treating this list as editable
    // would word-diff the old list against the new one, which was superlinear
    // and made the first keystroke hang for seconds on a real-sized history.
    // The bound is loose so this fails on that regression, not on a slow
    // machine.
    let commands = [
        "git status",
        "cargo build",
        "ls -la",
        "grep -rn TODO src",
        "make",
    ];
    let mut history = String::new();
    for i in 0..5000 {
        history.push_str(&format!(
            "- cmd: {} {}\n  when: {}\n",
            commands[i % commands.len()],
            i,
            i
        ));
    }
    let (mut app, mut io, window_id) = choose_command_app(&history);
    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, LIST).lines().count(), 5000);

    // The first keystroke is the expensive one: it shrinks the list most.
    let start = Instant::now();
    common::text_input(&mut app, &mut io, window_id, "g");
    common::tick(&mut app, &mut io);
    let elapsed = start.elapsed();

    // "git", "cargo" and "grep" all contain a g; "ls -la" and "make" do not.
    assert_eq!(buffer_text(&app, LIST).lines().count(), 3000);
    assert!(
        elapsed < Duration::from_secs(2),
        "first keystroke took {:?}",
        elapsed
    );
    app.assert_invariants();
}

// ── runner page ────────────────────────────────────────────────────────────────

#[test]
fn output_is_streamed_into_the_output_buffer() {
    let (mut app, mut io, _window_id) = runner_app("cargo build");

    io.processes[0].pending_output.extend_from_slice(b"one\n");
    common::tick(&mut app, &mut io);
    io.processes[0].pending_output.extend_from_slice(b"two\n");
    common::tick(&mut app, &mut io);

    assert_eq!(buffer_text(&app, OUTPUT), "one\ntwo\n");
    app.assert_invariants();
}

#[test]
fn status_bar_shows_command_and_elapsed_time() {
    let (mut app, mut io, _window_id) = runner_app("cargo build");
    common::tick(&mut app, &mut io);
    assert_eq!(
        buffer_text(&app, MAKER_STATUS),
        "cargo build (started 0s ago)"
    );

    // Elapsed time ticks up as frames pass.
    io.frame_start += Duration::from_secs(42);
    common::tick(&mut app, &mut io);
    assert_eq!(
        buffer_text(&app, MAKER_STATUS),
        "cargo build (started 42s ago)"
    );

    // Minutes and hours coarsen the resolution.
    io.frame_start += Duration::from_secs(3 * 60);
    common::tick(&mut app, &mut io);
    assert_eq!(
        buffer_text(&app, MAKER_STATUS),
        "cargo build (started 3m42s ago)"
    );
    io.frame_start += Duration::from_secs(60 * 60);
    common::tick(&mut app, &mut io);
    assert_eq!(
        buffer_text(&app, MAKER_STATUS),
        "cargo build (started 1h03m ago)"
    );
    app.assert_invariants();
}

#[test]
fn status_bar_elapsed_time_resets_on_restart() {
    let (mut app, mut io, window_id) = runner_app("cargo build");
    io.frame_start += Duration::from_secs(42);
    common::tick(&mut app, &mut io);
    assert_eq!(
        buffer_text(&app, MAKER_STATUS),
        "cargo build (started 42s ago)"
    );

    // Restart via ctrl+r; the elapsed time starts over.
    common::control_key(&mut app, &mut io, window_id, Key::Character("r"));
    common::tick(&mut app, &mut io);
    assert_eq!(
        buffer_text(&app, MAKER_STATUS),
        "cargo build (started 0s ago)"
    );
    app.assert_invariants();
}

#[test]
fn location_split_across_polls_is_jumpable() {
    let (mut app, mut io, window_id) = runner_app("cargo build");
    insert_file(&mut io, "/foo/bar.rs", "abc\ndefgh\n");

    // The path:line:col token arrives split across two polls.
    io.processes[0]
        .pending_output
        .extend_from_slice(b"foo/bar.rs");
    common::tick(&mut app, &mut io);
    io.processes[0]
        .pending_output
        .extend_from_slice(b":2:3 bad\n");
    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, OUTPUT), "foo/bar.rs:2:3 bad\n");

    // The cursor followed the appended output; move it up onto the report.
    common::control_key(&mut app, &mut io, window_id, Key::Character("i"));

    // Ctrl+enter on the report opens the file at line 2, column 3.
    let buffers_before = app.buffers.keys().count();
    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::char_input(&mut app, &mut io, window_id, 'X');
    assert_eq!(buffer_text(&app, buffers_before), "abc\ndeXfgh\n");

    // Ctrl+q returns to the runner output, where ctrl+r restarts the command.
    common::control_key(&mut app, &mut io, window_id, Key::Character("q"));
    common::control_key(&mut app, &mut io, window_id, Key::Character("r"));
    assert_eq!(io.processes.len(), 2);
    assert_eq!(buffer_text(&app, OUTPUT), "");
    app.assert_invariants();
}

#[test]
fn jump_with_default_col_lands_at_line_start() {
    let (mut app, mut io, window_id) = runner_app("cargo build");
    insert_file(&mut io, "/bar.rs", "abc\ndefgh\n");

    io.processes[0]
        .pending_output
        .extend_from_slice(b"bar.rs:2 bad\n");
    common::tick(&mut app, &mut io);

    // The cursor followed the appended output; move it up onto the report.
    common::control_key(&mut app, &mut io, window_id, Key::Character("i"));

    // No explicit column, so the jump lands at the start of line 2.
    let buffers_before = app.buffers.keys().count();
    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::char_input(&mut app, &mut io, window_id, 'X');
    assert_eq!(buffer_text(&app, buffers_before), "abc\nXdefgh\n");
    app.assert_invariants();
}

#[test]
fn location_with_a_colon_before_the_message_is_jumpable() {
    // gcc without a column, grep -n and make all print `path:line: msg`.
    let (mut app, mut io, window_id) = runner_app("make");
    insert_file(&mut io, "/bar.rs", "abc\ndefgh\n");

    io.processes[0]
        .pending_output
        .extend_from_slice(b"bar.rs:2: error: bad\n");
    common::tick(&mut app, &mut io);

    common::control_key(&mut app, &mut io, window_id, Key::Character("i"));
    let buffers_before = app.buffers.keys().count();
    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::char_input(&mut app, &mut io, window_id, 'X');
    assert_eq!(buffer_text(&app, buffers_before), "abc\nXdefgh\n");
    app.assert_invariants();
}

// One `path:line` report per line, padded so that a megabyte of output is
// a manageable number of parsed locations.
fn report_lines(report: &str, lines: usize) -> Vec<u8> {
    let mut line = report.as_bytes().to_vec();
    line.resize(100, b'x');
    line.push(b'\n');
    let mut out = Vec::with_capacity(lines * line.len());
    for _ in 0..lines {
        out.extend_from_slice(&line);
    }
    out
}

#[test]
fn locations_shift_when_old_output_is_trimmed() {
    let (mut app, mut io, window_id) = runner_app("cargo build");
    insert_file(&mut io, "/a.rs", "one\ntwo\n");
    insert_file(&mut io, "/c.rs", "nope\n");

    // Three rounds of output, sized so only the third crosses the 2MB trim
    // threshold, and so the megabyte that survives holds no c.rs reports.
    // The a.rs reports were parsed a tick before that trim, so they still
    // point at their own text only if the trim shifted them.
    for (report, lines) in [("c.rs:9 ", 11000), ("a.rs:1 ", 9000), ("b.rs:2 ", 1500)] {
        io.processes[0]
            .pending_output
            .extend_from_slice(&report_lines(report, lines));
        common::tick(&mut app, &mut io);
    }
    let output = buffer_text(&app, OUTPUT);
    assert!(output.len() <= 1024 * 1024, "not trimmed: {}", output.len());
    assert!(
        output.starts_with("a.rs:1 "),
        "trim kept the wrong text: {:?}",
        &output[..16]
    );

    // The report on the first line still opens a.rs, not the c.rs text that
    // used to live at those offsets.
    common::alt_key(&mut app, &mut io, window_id, Key::Character("i"));
    let buffers_before = app.buffers.keys().count();
    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::char_input(&mut app, &mut io, window_id, 'X');
    assert_eq!(buffer_text(&app, buffers_before), "Xone\ntwo\n");
    app.assert_invariants();
}

#[test]
fn relative_and_absolute_paths_resolve_against_the_command_dir() {
    let (mut app, mut io, window_id) = common::file_app(PathBuf::from("/repo/app/main.rs"), "x");
    io.git_roots.push(PathBuf::from("/repo/app"));
    insert_file(&mut io, "/repo/sibling/foo.rs", "one\ntwo\nthree\n");
    insert_file(&mut io, "/abs/path/bar.rs", "a\nb\nc\nqwerty\n");

    // ctrl+m → dir picker (default: repo root of current file = /repo/app)
    // alt+enter → command picker rooted at /repo/app
    common::control_key(&mut app, &mut io, window_id, Key::Character("m"));
    common::alt_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::text_input(&mut app, &mut io, window_id, "make");
    common::alt_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    assert_eq!(io.processes[0].dir, PathBuf::from("/repo/app"));

    io.processes[0]
        .pending_output
        .extend_from_slice(b"../sibling/foo.rs:3 bad\n/abs/path/bar.rs:4:5 worse\n");
    common::tick(&mut app, &mut io);

    // `..` resolves against the command dir: /repo/app/../sibling/foo.rs.
    common::control_key(&mut app, &mut io, window_id, Key::Character("i"));
    common::control_key(&mut app, &mut io, window_id, Key::Character("i"));
    let buffers_before = app.buffers.keys().count();
    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::char_input(&mut app, &mut io, window_id, 'X');
    assert_eq!(buffer_text(&app, buffers_before), "one\ntwo\nXthree\n");

    // An absolute path is used as-is, ignoring the command dir.
    common::control_key(&mut app, &mut io, window_id, Key::Character("q"));
    common::control_key(&mut app, &mut io, window_id, Key::Character("k"));
    let buffers_before = app.buffers.keys().count();
    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::char_input(&mut app, &mut io, window_id, 'X');
    assert_eq!(buffer_text(&app, buffers_before), "a\nb\nc\nqwerXty\n");
    app.assert_invariants();
}

#[test]
fn saving_a_file_buffer_restarts_the_command() {
    let (mut app, mut io, _window_id) = runner_app("cargo build");
    io.processes[0].pending_output.extend_from_slice(b"stale\n");
    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, OUTPUT), "stale\n");

    // Open the file in a second window and load it.
    insert_file(&mut io, "/src.txt", "contents");
    let buffer_id = buffer::from_file(&mut app, &mut io, PathBuf::from("/src.txt"));
    let window2_id = window::open_edit(&mut app, &mut io, buffer_id);
    io.frame_start += Duration::from_secs(1);
    common::tick(&mut app, &mut io);

    // Modify and save it.
    io.frame_start += Duration::from_secs(1);
    common::tick(&mut app, &mut io);
    common::char_input(&mut app, &mut io, window2_id, 'X');
    common::control_key(&mut app, &mut io, window2_id, Key::Character("s"));
    common::tick(&mut app, &mut io);

    // The old process is killed and the command respawned with a cleared
    // output buffer.
    assert!(io.processes[0].killed);
    assert_eq!(io.processes.len(), 2);
    assert_eq!(io.processes[1].command, "cargo build");
    assert!(!io.processes[1].killed);
    assert_eq!(buffer_text(&app, OUTPUT), "");
    app.assert_invariants();
}

#[test]
fn saving_an_unmodified_buffer_does_not_restart() {
    let (mut app, mut io, _window_id) = runner_app("cargo build");

    insert_file(&mut io, "/src.txt", "contents");
    let buffer_id = buffer::from_file(&mut app, &mut io, PathBuf::from("/src.txt"));
    let window2_id = window::open_edit(&mut app, &mut io, buffer_id);
    io.frame_start += Duration::from_secs(1);
    common::tick(&mut app, &mut io);

    // Save without modifying: a no-op, so the command keeps running.
    io.frame_start += Duration::from_secs(1);
    common::tick(&mut app, &mut io);
    common::control_key(&mut app, &mut io, window2_id, Key::Character("s"));
    common::tick(&mut app, &mut io);

    assert_eq!(io.processes.len(), 1);
    assert!(!io.processes[0].killed);
    app.assert_invariants();
}

#[test]
fn ctrl_r_restarts_the_command() {
    let (mut app, mut io, window_id) = runner_app("cargo build");
    io.processes[0].pending_output.extend_from_slice(b"stale\n");
    common::tick(&mut app, &mut io);

    common::control_key(&mut app, &mut io, window_id, Key::Character("r"));

    assert!(io.processes[0].killed);
    assert_eq!(io.processes.len(), 2);
    assert_eq!(buffer_text(&app, OUTPUT), "");
    app.assert_invariants();
}

#[test]
fn exit_is_reported_once() {
    let (mut app, mut io, _window_id) = runner_app("cargo build");

    io.processes[0].pending_output.extend_from_slice(b"done\n");
    io.processes[0].exit_code = Some(1);
    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, OUTPUT), "done\n\n[exited: 1]\n");

    // Further ticks don't repeat the exited line.
    common::tick(&mut app, &mut io);
    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, OUTPUT), "done\n\n[exited: 1]\n");
    app.assert_invariants();
}

#[test]
fn typing_into_the_output_does_nothing() {
    let (mut app, mut io, window_id) = runner_app("cargo build");
    insert_file(&mut io, "/bar.rs", "abc\ndefgh\n");
    io.processes[0]
        .pending_output
        .extend_from_slice(b"foo.rs:1 bad\n");
    common::tick(&mut app, &mut io);

    // The output is generated, so every key that would edit it is ignored.
    io.clipboard = Some("paste".into());
    common::text_input(&mut app, &mut io, window_id, "hello\n");
    common::key(
        &mut app,
        &mut io,
        window_id,
        Key::Named(NamedKey::Backspace),
    );
    common::key(&mut app, &mut io, window_id, Key::Named(NamedKey::Delete));
    common::control_key(&mut app, &mut io, window_id, Key::Character("x"));
    common::control_key(&mut app, &mut io, window_id, Key::Character("v"));
    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, OUTPUT), "foo.rs:1 bad\n");

    // Later output still parses and jumps.
    io.processes[0]
        .pending_output
        .extend_from_slice(b"bar.rs:2:3 x\n");
    common::tick(&mut app, &mut io);
    common::control_key(&mut app, &mut io, window_id, Key::Character("i"));
    let buffers_before = app.buffers.keys().count();
    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::char_input(&mut app, &mut io, window_id, 'X');
    assert_eq!(buffer_text(&app, buffers_before), "abc\ndeXfgh\n");
    app.assert_invariants();
}

#[test]
fn the_output_can_be_selected_and_copied() {
    let (mut app, mut io, window_id) = runner_app("cargo build");
    io.processes[0].pending_output.extend_from_slice(b"one\n");
    common::tick(&mut app, &mut io);

    // Reading the output is the point of showing it in an editor, so
    // selection and copy still work.
    common::alt_key(&mut app, &mut io, window_id, Key::Character("i"));
    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Space));
    for _ in 0..3 {
        common::control_key(&mut app, &mut io, window_id, Key::Character("l"));
    }
    common::control_key(&mut app, &mut io, window_id, Key::Character("c"));
    assert_eq!(io.clipboard, Some("one".into()));

    // Cut would delete, so it does nothing.
    common::control_key(&mut app, &mut io, window_id, Key::Character("x"));
    assert_eq!(buffer_text(&app, OUTPUT), "one\n");
    app.assert_invariants();
}

#[test]
fn output_is_drained_and_restarted_while_an_edit_page_is_on_top() {
    let (mut app, mut io, window_id) = runner_app("cargo build");
    insert_file(&mut io, "/foo.rs", "abc\n");
    io.processes[0]
        .pending_output
        .extend_from_slice(b"foo.rs:1 bad\n");
    common::tick(&mut app, &mut io);

    // Jump to the report, pushing an edit page over the runner page.
    common::control_key(&mut app, &mut io, window_id, Key::Character("i"));
    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    io.frame_start += Duration::from_secs(1);
    common::tick(&mut app, &mut io);

    // Output keeps flowing into the hidden runner page.
    io.processes[0].pending_output.extend_from_slice(b"more\n");
    common::tick(&mut app, &mut io);
    assert!(io.processes[0].pending_output.is_empty());
    assert_eq!(buffer_text(&app, OUTPUT), "foo.rs:1 bad\nmore\n");

    // Saving from the edit page restarts the command straight away.
    io.frame_start += Duration::from_secs(1);
    common::tick(&mut app, &mut io);
    common::char_input(&mut app, &mut io, window_id, 'X');
    common::control_key(&mut app, &mut io, window_id, Key::Character("s"));
    common::tick(&mut app, &mut io);
    assert!(io.processes[0].killed);
    assert_eq!(io.processes.len(), 2);
    assert_eq!(buffer_text(&app, OUTPUT), "");

    // Back on the runner page, the new run's output is there.
    io.processes[1].pending_output.extend_from_slice(b"fresh\n");
    common::tick(&mut app, &mut io);
    common::control_key(&mut app, &mut io, window_id, Key::Character("q"));
    assert_eq!(buffer_text(&app, OUTPUT), "fresh\n");
    app.assert_invariants();
}

#[test]
fn the_output_has_no_undo_history() {
    let (mut app, mut io, window_id) = runner_app("cargo build");
    io.processes[0].pending_output.extend_from_slice(b"one\n");
    common::tick(&mut app, &mut io);
    io.processes[0].pending_output.extend_from_slice(b"two\n");
    common::tick(&mut app, &mut io);
    assert_eq!(buffer_text(&app, OUTPUT), "one\ntwo\n");

    // Nothing the user did produced this text, so there is nothing to undo
    // or redo - and no history piling up behind a command that never stops
    // printing.
    common::control_key(&mut app, &mut io, window_id, Key::Character("z"));
    assert_eq!(buffer_text(&app, OUTPUT), "one\ntwo\n");
    common::control_key(&mut app, &mut io, window_id, Key::Character("Z"));
    assert_eq!(buffer_text(&app, OUTPUT), "one\ntwo\n");

    // A restart wipes the output.
    common::control_key(&mut app, &mut io, window_id, Key::Character("r"));
    assert_eq!(buffer_text(&app, OUTPUT), "");
    app.assert_invariants();
}

// Bytes shaped like a command producing endless output: mostly printable,
// with newlines, colons and digits so location parsing fires too.
fn junk(n: usize, seed: &mut u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let b = (*seed >> 33) as u8;
        out.push(match b % 8 {
            0 => b'\n',
            1 => b':',
            2..=3 => b'0' + (b % 10),
            _ => 0x20 + (b % 0x5e),
        });
    }
    out
}

#[test]
fn endless_output_is_capped_and_still_jumpable() {
    // `cat /dev/random` used to grow the output buffer until the editor
    // died. The oldest output is dropped instead, so the buffer stays
    // bounded however much the command produces.
    let (mut app, mut io, window_id) = runner_app("cat /dev/random");
    insert_file(&mut io, "/bar.rs", "abc\ndefgh\n");
    let mut seed = 1;
    for _ in 0..32 {
        let bytes = junk(128 * 1024, &mut seed);
        io.processes[0].pending_output.extend_from_slice(&bytes);
        common::tick(&mut app, &mut io);
        // MAX_OUTPUT_BYTES is 1MB and trimming starts at twice that.
        assert!(buffer_text(&app, OUTPUT).len() <= 2 * 1024 * 1024);
    }

    // A location reported after all that trimming still jumps.
    io.processes[0]
        .pending_output
        .extend_from_slice(b"\nbar.rs:2:3 bad\n");
    common::tick(&mut app, &mut io);
    common::control_key(&mut app, &mut io, window_id, Key::Character("i"));
    let buffers_before = app.buffers.keys().count();
    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::char_input(&mut app, &mut io, window_id, 'X');
    assert_eq!(buffer_text(&app, buffers_before), "abc\ndeXfgh\n");
    app.assert_invariants();
}

#[test]
fn ctrl_q_escapes_while_a_command_floods() {
    let (mut app, mut io, window_id) = runner_app("cat /dev/random");
    let mut seed = 1;
    for _ in 0..8 {
        let bytes = junk(128 * 1024, &mut seed);
        io.processes[0].pending_output.extend_from_slice(&bytes);
        common::tick(&mut app, &mut io);
    }

    // Ctrl+q still gets through, pops the page and kills the command.
    common::control_key(&mut app, &mut io, window_id, Key::Character("q"));
    assert!(io.processes[0].killed);
    common::char_input(&mut app, &mut io, window_id, 'W');
    assert_eq!(buffer_text(&app, 0), "W");
    app.assert_invariants();
}

#[test]
fn ctrl_q_from_the_output_skips_the_picker() {
    let (mut app, mut io, window_id) = runner_app("cargo build");

    // The pickers were replaced by the runner page, so ctrl+q goes straight
    // back to the original scratch page.
    common::control_key(&mut app, &mut io, window_id, Key::Character("q"));
    common::char_input(&mut app, &mut io, window_id, 'W');

    assert_eq!(buffer_text(&app, 0), "W");
    // Popping the runner page kills its process.
    assert!(io.processes[0].killed);
    app.assert_invariants();
}
