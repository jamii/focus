// The diff page: ctrl+2 to open it, ctrl+enter to jump from a hunk to the
// file it came from.

use std::path::PathBuf;

use focus_core::app::{App, VcsChange, VcsFile, VcsFileKind, VcsHunk, VcsLine, VcsLineKind};
use focus_core::fuzz::MockIO;
use focus_core::input::{Key, NamedKey};
use focus_core::window::WindowId;

mod common;

// Buffer creation order: file_app makes the file buffer (0) and the status
// bar buffer (1); ctrl+2 then makes the diff page's buffer (2); ctrl+enter
// replaces the page under it with a new edit page, which shares the file
// buffer and makes a status bar of its own (3). The first status bar stops
// being updated as soon as the diff page is pushed over it, so what the
// page ctrl+enter opened is showing is buffer 3.
const FILE: usize = 0;
const DIFF: usize = 2;
const OPENED_STATUS_BAR: usize = 3;

const ROOT: &str = "/repo";
const PATH: &str = "/repo/file.txt";
const TEXT: &str = "one\nTWO\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n";

fn buffer_text(app: &App, n: usize) -> String {
    app.buffers.keys().nth(n).unwrap().text(app).to_string()
}

fn line(kind: VcsLineKind, text: &str) -> VcsLine {
    VcsLine {
        kind,
        text: text.into(),
    }
}

// One modified file, changed on its second line, and a second hunk further
// down so that a reveal has somewhere to land.
fn change() -> VcsChange {
    VcsChange {
        root: PathBuf::from(ROOT),
        change_id: "qpvuntsmwlqt".into(),
        commit_id: "1f2a3b4c5d6e".into(),
        author: "Jamie <jamie@example.com> (2026-09-14 15:30:00)".into(),
        description: "a change\nwith two lines".into(),
        files: vec![VcsFile {
            relative_path: PathBuf::from("file.txt"),
            kind: VcsFileKind::Modified,
            binary: false,
            hunks: vec![
                VcsHunk {
                    old_lines: 0..3,
                    new_lines: 0..3,
                    lines: vec![
                        line(VcsLineKind::Context, "one"),
                        line(VcsLineKind::Removed, "two"),
                        line(VcsLineKind::Added, "TWO"),
                        line(VcsLineKind::Context, "three"),
                    ],
                },
                VcsHunk {
                    old_lines: 5..7,
                    new_lines: 5..8,
                    lines: vec![
                        line(VcsLineKind::Context, "six"),
                        line(VcsLineKind::Added, "seven"),
                        line(VcsLineKind::Context, "eight"),
                    ],
                },
            ],
        }],
    }
}

fn repo_app(change: Option<VcsChange>) -> (App, MockIO, WindowId) {
    let (mut app, mut io, window_id) = common::file_app(PathBuf::from(PATH), TEXT);
    io.repo_roots.push(PathBuf::from(ROOT));
    if let Some(change) = change {
        io.vcs_changes.insert(PathBuf::from(ROOT), change);
    }
    common::tick(&mut app, &mut io);
    (app, io, window_id)
}

// Move the cursor to `line`, 0-based, from the top of the file.
fn cursor_to_line(app: &mut App, io: &mut MockIO, window_id: WindowId, line: usize) {
    common::alt_key(app, io, window_id, Key::Character("i"));
    for _ in 0..line {
        common::control_key(app, io, window_id, Key::Character("k"));
    }
}

fn open_diff(app: &mut App, io: &mut MockIO, window_id: WindowId) {
    common::control_key(app, io, window_id, Key::Character("2"));
    common::tick(app, io);
}

#[test]
fn ctrl_2_shows_the_change() {
    let (mut app, mut io, window_id) = repo_app(Some(change()));

    open_diff(&mut app, &mut io, window_id);

    assert_eq!(
        buffer_text(&app, DIFF),
        "\
Change:  qpvuntsmwlqt
Commit:  1f2a3b4c5d6e
Author:  Jamie <jamie@example.com> (2026-09-14 15:30:00)

    a change
    with two lines

M file.txt
  @@ -1,3 +1,3 @@
    1     1   one
    2       - two
          2 + TWO
    3     3   three
  @@ -6,2 +6,3 @@
    6     6   six
          7 + seven
    7     8   eight"
    );
    app.assert_invariants();
}

#[test]
fn ctrl_enter_opens_the_file_at_the_line_under_the_cursor() {
    let (mut app, mut io, window_id) = repo_app(Some(change()));
    // Line 6 of the file (0-based), inside the second hunk.
    cursor_to_line(&mut app, &mut io, window_id, 6);
    open_diff(&mut app, &mut io, window_id);

    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::tick(&mut app, &mut io);

    // The diff page opened on the hunk holding line 6, and ctrl+enter
    // followed that back to the file.
    assert_eq!(buffer_text(&app, OPENED_STATUS_BAR), "/repo/file.txt 7:1");
    assert_eq!(io.open_windows.len(), 1);
    app.assert_invariants();
}

#[test]
fn ctrl_2_lands_on_the_first_hunk_when_the_cursor_is_past_them_all() {
    let (mut app, mut io, window_id) = repo_app(Some(change()));
    // Line 9 is below the last hunk, so there is nothing at or after it.
    cursor_to_line(&mut app, &mut io, window_id, 9);
    open_diff(&mut app, &mut io, window_id);

    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::tick(&mut app, &mut io);

    assert_eq!(buffer_text(&app, OPENED_STATUS_BAR), "/repo/file.txt 1:1");
    app.assert_invariants();
}

#[test]
fn ctrl_shift_enter_opens_the_file_in_a_new_window() {
    let (mut app, mut io, window_id) = repo_app(Some(change()));
    cursor_to_line(&mut app, &mut io, window_id, 1);
    open_diff(&mut app, &mut io, window_id);

    common::control_shift_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::tick(&mut app, &mut io);

    assert_eq!(io.open_windows.len(), 2);
    // The original window still shows the diff.
    assert!(buffer_text(&app, DIFF).starts_with("Change:  qpvuntsmwlqt"));
    app.assert_invariants();
}

#[test]
fn a_deleted_file_opens_nothing() {
    let mut change = change();
    change.files[0].kind = VcsFileKind::Deleted;
    let (mut app, mut io, window_id) = repo_app(Some(change));
    open_diff(&mut app, &mut io, window_id);
    let buffers = app.buffers.keys().count();

    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::tick(&mut app, &mut io);

    assert_eq!(app.buffers.keys().count(), buffers);
    app.assert_invariants();
}

#[test]
fn the_header_lines_open_nothing() {
    let (mut app, mut io, window_id) = repo_app(Some(change()));
    open_diff(&mut app, &mut io, window_id);
    // Back to the top of the page, above the first file.
    common::alt_key(&mut app, &mut io, window_id, Key::Character("i"));
    common::tick(&mut app, &mut io);
    let buffers = app.buffers.keys().count();

    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::tick(&mut app, &mut io);

    assert_eq!(app.buffers.keys().count(), buffers);
    app.assert_invariants();
}

#[test]
fn a_repo_that_cannot_be_read_shows_the_error() {
    let (mut app, mut io, window_id) = repo_app(None);

    open_diff(&mut app, &mut io, window_id);

    assert_eq!(buffer_text(&app, DIFF), "/repo: no repo");
    app.assert_invariants();
}

#[test]
fn the_page_follows_the_change() {
    let (mut app, mut io, window_id) = repo_app(Some(change()));
    open_diff(&mut app, &mut io, window_id);
    assert!(buffer_text(&app, DIFF).contains("+ TWO"));

    let mut changed = change();
    changed.files[0].hunks[0].lines[2] = line(VcsLineKind::Added, "TWO AND A HALF");
    io.vcs_changes.insert(PathBuf::from(ROOT), changed);
    common::tick(&mut app, &mut io);

    assert!(buffer_text(&app, DIFF).contains("+ TWO AND A HALF"));
    assert_eq!(buffer_text(&app, FILE), TEXT);
    app.assert_invariants();
}
