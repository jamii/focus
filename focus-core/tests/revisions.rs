// The revision picker (alt+2), and the diff page showing a revision other
// than the working copy - which has to be checked out before there is
// anything on disk to jump into.

use std::path::PathBuf;

use focus_core::app::{
    App, VcsChange, VcsFile, VcsFileKind, VcsHunk, VcsLine, VcsLineKind, VcsRevision, VcsRevisionId,
};
use focus_core::fuzz::MockIO;
use focus_core::input::{Key, NamedKey};
use focus_core::window::WindowId;

mod common;

// Buffer creation order: file_app makes the file buffer (0) and its status
// bar (1); alt+2 makes the picker's preview (2), search field (3) and list
// (4); ctrl+enter there makes the diff page's buffer (5) and status bar
// (6); ctrl+enter there opens an edit page, which shares the file buffer
// and makes a status bar of its own (7).
const PICKER_PREVIEW: usize = 2;
const PICKER_LIST: usize = 4;
const DIFF: usize = 5;
const DIFF_STATUS_BAR: usize = 6;
const OPENED_STATUS_BAR: usize = 7;

const ROOT: &str = "/repo";
const PATH: &str = "/repo/file.txt";
const TEXT: &str = "one\ntwo\nthree\n";

// Change ids are the full reverse hex jj reports; the picker and the
// status bar show the first twelve characters.
const WORKING_COPY_ID: &str = "qpvuntsmwlqtqpvuntsmwlqtqpvuntsm";
const BASE_ID: &str = "kkmpptxzrspxkkmpptxzrspxkkmpptxz";

fn buffer_text(app: &App, n: usize) -> String {
    app.buffers.keys().nth(n).unwrap().text(app).to_string()
}

fn revision(change_id: &str, description: &str, is_working_copy: bool) -> VcsRevision {
    VcsRevision {
        change_id: change_id.into(),
        commit_id: "1f2a3b4c5d6e7f80".into(),
        author: "Jamie <jamie@example.com> (2026-09-14 15:30:00)".into(),
        description: description.into(),
        is_working_copy,
    }
}

// A change touching `file.txt` on its second line.
fn change(description: &str, text: &str) -> VcsChange {
    VcsChange {
        root: PathBuf::from(ROOT),
        change_id: "qpvuntsmwlqt".into(),
        commit_id: "1f2a3b4c5d6e".into(),
        author: "Jamie <jamie@example.com> (2026-09-14 15:30:00)".into(),
        description: description.into(),
        files: vec![VcsFile {
            relative_path: PathBuf::from("file.txt"),
            kind: VcsFileKind::Modified,
            binary: false,
            hunks: vec![VcsHunk {
                old_lines: 0..3,
                new_lines: 0..3,
                lines: vec![
                    VcsLine {
                        kind: VcsLineKind::Context,
                        text: "one".into(),
                    },
                    VcsLine {
                        kind: VcsLineKind::Removed,
                        text: "old".into(),
                    },
                    VcsLine {
                        kind: VcsLineKind::Added,
                        text: text.into(),
                    },
                    VcsLine {
                        kind: VcsLineKind::Context,
                        text: "three".into(),
                    },
                ],
            }],
        }],
    }
}

fn repo_app() -> (App, MockIO, WindowId) {
    let (mut app, mut io, window_id) = common::file_app(PathBuf::from(PATH), TEXT);
    io.repo_roots.push(PathBuf::from(ROOT));
    io.vcs_revisions.insert(
        PathBuf::from(ROOT),
        vec![
            revision(WORKING_COPY_ID, "the change under test", true),
            revision(BASE_ID, "base", false),
        ],
    );
    io.vcs_changes.insert(
        (PathBuf::from(ROOT), VcsRevisionId::WorkingCopy),
        change("the change under test", "two"),
    );
    io.vcs_changes.insert(
        (PathBuf::from(ROOT), VcsRevisionId::Change(BASE_ID.into())),
        change("base", "from the base"),
    );
    common::tick(&mut app, &mut io);
    (app, io, window_id)
}

fn open_picker(app: &mut App, io: &mut MockIO, window_id: WindowId) {
    common::alt_key(app, io, window_id, Key::Character("2"));
    common::tick(app, io);
}

// Put the diff page's cursor on the line holding `needle`, which is where
// ctrl+enter reads its destination from.
fn cursor_to_line(app: &mut App, io: &mut MockIO, window_id: WindowId, needle: &str) {
    let text = buffer_text(app, DIFF);
    let line = text
        .lines()
        .position(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("no {needle:?} in\n{text}"));
    for _ in 0..line {
        common::control_key(app, io, window_id, Key::Character("k"));
    }
}

// Choose the `n`th revision in the list.
fn choose(app: &mut App, io: &mut MockIO, window_id: WindowId, n: usize) {
    for _ in 0..n {
        common::control_key(app, io, window_id, Key::Character("k"));
    }
    common::control_key(app, io, window_id, Key::Named(NamedKey::Enter));
    common::tick(app, io);
}

#[test]
fn alt_2_lists_the_revisions() {
    let (mut app, mut io, window_id) = repo_app();

    open_picker(&mut app, &mut io, window_id);

    assert_eq!(
        buffer_text(&app, PICKER_LIST),
        "\
@ qpvuntsmwlqt the change under test
  kkmpptxzrspx base"
    );
    app.assert_invariants();
}

#[test]
fn typing_filters_the_list() {
    let (mut app, mut io, window_id) = repo_app();
    open_picker(&mut app, &mut io, window_id);

    common::text_input(&mut app, &mut io, window_id, "base");
    common::tick(&mut app, &mut io);

    assert_eq!(buffer_text(&app, PICKER_LIST), "  kkmpptxzrspx base");
    app.assert_invariants();
}

#[test]
fn a_repo_that_cannot_be_read_shows_the_error() {
    let (mut app, mut io, window_id) = repo_app();
    io.vcs_revisions.remove(&PathBuf::from(ROOT));

    open_picker(&mut app, &mut io, window_id);

    assert_eq!(buffer_text(&app, PICKER_LIST), "/repo: no repo");
    app.assert_invariants();
}

#[test]
fn choosing_a_revision_opens_its_diff_page() {
    let (mut app, mut io, window_id) = repo_app();
    open_picker(&mut app, &mut io, window_id);

    choose(&mut app, &mut io, window_id, 1);

    assert_eq!(buffer_text(&app, DIFF_STATUS_BAR), "jj show kkmpptxzrspx");
    assert!(
        buffer_text(&app, DIFF).contains("+ from the base"),
        "{}",
        buffer_text(&app, DIFF)
    );
    app.assert_invariants();
}

#[test]
fn choosing_the_working_copy_opens_the_page_ctrl_2_would() {
    let (mut app, mut io, window_id) = repo_app();
    open_picker(&mut app, &mut io, window_id);

    choose(&mut app, &mut io, window_id, 0);

    assert_eq!(buffer_text(&app, DIFF_STATUS_BAR), "jj show @");
    assert!(buffer_text(&app, DIFF).contains("+ two"));
    app.assert_invariants();
}

#[test]
fn jumping_into_another_revision_checks_it_out_first() {
    let (mut app, mut io, window_id) = repo_app();
    io.vcs_checkout_pending = true;
    open_picker(&mut app, &mut io, window_id);
    choose(&mut app, &mut io, window_id, 1);

    cursor_to_line(&mut app, &mut io, window_id, "+ from the base");
    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::tick(&mut app, &mut io);

    assert_eq!(
        io.vcs_checkouts.last(),
        Some(&(PathBuf::from(ROOT), VcsRevisionId::Change(BASE_ID.into())))
    );
    // Still on the diff page, waiting.
    assert_eq!(
        buffer_text(&app, DIFF_STATUS_BAR),
        "checking out kkmpptxzrspx ..."
    );
    let buffers = app.buffers.keys().count();

    // The checkout lands, and the jump it was holding up happens. The
    // page it opens fills its own status bar on the tick after that.
    io.vcs_checkout_pending = false;
    common::tick(&mut app, &mut io);
    common::tick(&mut app, &mut io);

    assert_eq!(app.buffers.keys().count(), buffers + 1);
    assert_eq!(buffer_text(&app, OPENED_STATUS_BAR), "/repo/file.txt 2:1");
    app.assert_invariants();
}

#[test]
fn jumping_from_the_working_copy_checks_nothing_out() {
    let (mut app, mut io, window_id) = repo_app();
    open_picker(&mut app, &mut io, window_id);
    choose(&mut app, &mut io, window_id, 0);

    cursor_to_line(&mut app, &mut io, window_id, "+ two");
    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::tick(&mut app, &mut io);

    // The working copy's files are the ones already on disk.
    assert_eq!(io.vcs_checkouts, vec![]);
    assert_eq!(buffer_text(&app, OPENED_STATUS_BAR), "/repo/file.txt 2:1");
    app.assert_invariants();
}

#[test]
fn a_checkout_that_fails_says_so() {
    let (mut app, mut io, window_id) = repo_app();
    io.vcs_checkout_error = Some("the working copy is stale".to_string());
    open_picker(&mut app, &mut io, window_id);
    choose(&mut app, &mut io, window_id, 1);
    let buffers = app.buffers.keys().count();

    cursor_to_line(&mut app, &mut io, window_id, "+ from the base");
    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::tick(&mut app, &mut io);

    assert_eq!(
        buffer_text(&app, DIFF_STATUS_BAR),
        "the working copy is stale"
    );
    // Nothing was opened.
    assert_eq!(app.buffers.keys().count(), buffers);
    app.assert_invariants();
}

#[test]
fn the_picker_follows_the_revisions() {
    let (mut app, mut io, window_id) = repo_app();
    open_picker(&mut app, &mut io, window_id);

    // A new revision appears - someone ran `jj new` in a terminal.
    io.vcs_revisions.insert(
        PathBuf::from(ROOT),
        vec![
            revision("mzvwutvlkqwtmzvwutvlkqwtmzvwutvl", "brand new", true),
            revision(WORKING_COPY_ID, "the change under test", false),
            revision(BASE_ID, "base", false),
        ],
    );
    common::tick(&mut app, &mut io);

    assert_eq!(
        buffer_text(&app, PICKER_LIST),
        "\
@ mzvwutvlkqwt brand new
  qpvuntsmwlqt the change under test
  kkmpptxzrspx base"
    );
    app.assert_invariants();
}

#[test]
fn the_preview_shows_the_selected_revisions_change() {
    let (mut app, mut io, window_id) = repo_app();

    open_picker(&mut app, &mut io, window_id);

    // The selection starts on the working copy, so that is what the
    // preview shows - the page ctrl+2 would open, without its status bar.
    assert_eq!(
        buffer_text(&app, PICKER_PREVIEW),
        "\
Change:  qpvuntsmwlqt
Commit:  1f2a3b4c5d6e
Author:  Jamie <jamie@example.com> (2026-09-14 15:30:00)

    the change under test

M file.txt
  @@ -1,3 +1,3 @@
    1     1   one
    2       - old
          2 + two
    3     3   three"
    );
    app.assert_invariants();
}

#[test]
fn moving_the_selection_moves_the_preview() {
    let (mut app, mut io, window_id) = repo_app();
    open_picker(&mut app, &mut io, window_id);

    common::control_key(&mut app, &mut io, window_id, Key::Character("k"));
    common::tick(&mut app, &mut io);

    let preview = buffer_text(&app, PICKER_PREVIEW);
    assert!(preview.contains("    base"), "{preview}");
    assert!(preview.contains("+ from the base"), "{preview}");
    app.assert_invariants();
}

#[test]
fn the_preview_is_what_choosing_opens() {
    let (mut app, mut io, window_id) = repo_app();
    open_picker(&mut app, &mut io, window_id);
    common::control_key(&mut app, &mut io, window_id, Key::Character("k"));
    common::tick(&mut app, &mut io);
    let preview = buffer_text(&app, PICKER_PREVIEW);

    common::control_key(&mut app, &mut io, window_id, Key::Named(NamedKey::Enter));
    common::tick(&mut app, &mut io);

    assert_eq!(buffer_text(&app, DIFF), preview);
    app.assert_invariants();
}

#[test]
fn a_revision_that_cannot_be_read_shows_the_error_in_the_preview() {
    let (mut app, mut io, window_id) = repo_app();
    io.vcs_changes
        .remove(&(PathBuf::from(ROOT), VcsRevisionId::Change(BASE_ID.into())));
    open_picker(&mut app, &mut io, window_id);

    common::control_key(&mut app, &mut io, window_id, Key::Character("k"));
    common::tick(&mut app, &mut io);

    assert_eq!(buffer_text(&app, PICKER_PREVIEW), "/repo: no kkmpptxzrspx");
    app.assert_invariants();
}

#[test]
fn filtering_the_list_moves_the_preview_with_it() {
    let (mut app, mut io, window_id) = repo_app();
    open_picker(&mut app, &mut io, window_id);

    // One match left, and it is the one previewed.
    common::text_input(&mut app, &mut io, window_id, "base");
    common::tick(&mut app, &mut io);

    assert!(
        buffer_text(&app, PICKER_PREVIEW).contains("+ from the base"),
        "{}",
        buffer_text(&app, PICKER_PREVIEW)
    );
    app.assert_invariants();
}
