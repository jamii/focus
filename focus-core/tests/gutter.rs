// The left-gutter change bars, and what clicking one does.

use std::path::PathBuf;

use focus_core::app::{App, VcsChange, VcsChangeKind, VcsFileStatus, VcsLineRange};
use focus_core::drawing::{DrawCommand, Drawing, FULL_BLOCK};
use focus_core::fuzz::MockIO;
use focus_core::input::{ButtonState, Key, ModifiersState};
use focus_core::style::{VCS_ADDED_COLOR, VCS_DELETED_COLOR, VCS_MODIFIED_COLOR};
use focus_core::window::WindowId;

mod common;

const ROOT: &str = "/repo";
const PATH: &str = "/repo/file.txt";
const TEXT: &str = "one\ntwo\nthree\nfour\nfive\n";

const ROWS: usize = 10;
const WRAP_CHARS: usize = 20;

fn buffer_text(app: &App, n: usize) -> String {
    app.buffers.keys().nth(n).unwrap().text(app).to_string()
}

fn range(lines: std::ops::Range<usize>, kind: VcsChangeKind) -> VcsLineRange {
    VcsLineRange { lines, kind }
}

fn file_app(text: &str, ranges: Vec<VcsLineRange>) -> (App, MockIO, WindowId) {
    let (mut app, mut io, window_id) = common::file_app(PathBuf::from(PATH), text);
    io.repo_roots.push(PathBuf::from(ROOT));
    io.vcs_statuses
        .insert(PathBuf::from(PATH), VcsFileStatus { ranges });
    io.vcs_changes.insert(
        PathBuf::from(ROOT),
        VcsChange {
            root: PathBuf::from(ROOT),
            change_id: "qpvuntsmwlqt".into(),
            commit_id: "1f2a3b4c5d6e".into(),
            author: "Jamie <jamie@example.com> (2026-09-14 15:30:00)".into(),
            description: "a change".into(),
            files: Vec::new(),
        },
    );
    common::tick(&mut app, &mut io);
    (app, io, window_id)
}

// Every change bar drawn, as (x, top pixel, kind, height in pixels).
fn bars(app: &App, drawing: &Drawing) -> Vec<(f32, f32, &'static str, f32)> {
    let cell_w = app.cell_size()[0] as f32;
    let mut bars = Vec::new();
    for command in &drawing.commands {
        let DrawCommand::Character(character) = command else {
            continue;
        };
        let kind = match character.color {
            VCS_ADDED_COLOR => "added",
            VCS_MODIFIED_COLOR => "modified",
            VCS_DELETED_COLOR => "deleted",
            _ => continue,
        };
        assert_eq!(character.ch, FULL_BLOCK);
        // Half a cell wide, so the wrap and cursor markers drawn over the
        // bars stay readable.
        assert_eq!(character.dst.size[0], cell_w / 2.0);
        bars.push((
            character.dst.pos[0],
            character.dst.pos[1],
            kind,
            character.dst.size[1],
        ));
    }
    bars
}

// The change bar in each row of the left gutter, as (row, kind, how tall
// it is as a fraction of a cell).
fn gutter_bars(app: &App, drawing: &Drawing) -> Vec<(usize, &'static str, f32)> {
    let cell_h = app.cell_size()[1] as f32;
    bars(app, drawing)
        .into_iter()
        .filter(|(x, ..)| *x == 0.0)
        .map(|(_, y, kind, height)| ((y / cell_h).round() as usize, kind, height / cell_h))
        .collect()
}

// The change marks in the right-gutter scrollbar, as (top pixel, kind,
// height in pixels). Not rows: the whole file is scaled into the gutter.
fn scrollbar_marks(app: &App, drawing: &Drawing) -> Vec<(f32, &'static str, f32)> {
    bars(app, drawing)
        .into_iter()
        .filter(|(x, ..)| *x > 0.0)
        .map(|(_, y, kind, height)| (y, kind, height))
        .collect()
}

fn draw(app: &mut App, window_id: WindowId) -> Drawing {
    common::draw(app, window_id, WRAP_CHARS, ROWS)
}

// Press the mouse in the gutter of `row`, with whatever modifiers.
fn press_gutter(
    app: &mut App,
    io: &mut MockIO,
    window_id: WindowId,
    row: usize,
    modifiers: ModifiersState,
) {
    let [cell_w, cell_h] = [app.cell_size()[0] as f32, app.cell_size()[1] as f32];
    let position = [cell_w / 4.0, row as f32 * cell_h + cell_h / 2.0];
    common::mouse_enter(app, io, window_id);
    common::mouse_moved(app, io, window_id, position);
    common::modifiers(app, io, window_id, modifiers);
    common::mouse_button(app, io, window_id, ButtonState::Pressed, position);
    common::mouse_button(app, io, window_id, ButtonState::Released, position);
    common::modifiers(app, io, window_id, ModifiersState::default());
    common::tick(app, io);
}

#[test]
fn bars_mark_added_modified_and_deleted_lines() {
    let (mut app, mut io, window_id) = file_app(
        TEXT,
        vec![
            range(0..1, VcsChangeKind::Added),
            range(2..3, VcsChangeKind::Modified),
            // Lines were deleted between "four" and "five".
            range(4..4, VcsChangeKind::Deleted),
        ],
    );
    common::tick(&mut app, &mut io);

    let drawing = draw(&mut app, window_id);

    assert_eq!(
        gutter_bars(&app, &drawing),
        vec![
            (0, "added", 1.0),
            (2, "modified", 1.0),
            // A deletion has no line of its own, so it is a stub at the
            // top edge of the line it sits before.
            (4, "deleted", 0.25),
        ]
    );
    app.assert_invariants();
}

#[test]
fn a_soft_wrapped_line_is_marked_on_every_row_it_covers() {
    let long = "x".repeat(WRAP_CHARS + 5);
    let text = format!("{long}\nshort\n");
    let (mut app, mut io, window_id) = file_app(&text, vec![range(0..1, VcsChangeKind::Modified)]);
    common::tick(&mut app, &mut io);

    let drawing = draw(&mut app, window_id);

    assert_eq!(
        gutter_bars(&app, &drawing),
        vec![(0, "modified", 1.0), (1, "modified", 1.0)]
    );
    app.assert_invariants();
}

#[test]
fn an_unchanged_file_has_no_bars() {
    let (mut app, mut io, window_id) = common::file_app(PathBuf::from(PATH), TEXT);
    common::tick(&mut app, &mut io);

    let drawing = draw(&mut app, window_id);

    assert_eq!(gutter_bars(&app, &drawing), vec![]);
    app.assert_invariants();
}

#[test]
fn clicking_a_bar_opens_the_diff_page() {
    let (mut app, mut io, window_id) = file_app(TEXT, vec![range(2..3, VcsChangeKind::Modified)]);
    draw(&mut app, window_id);

    press_gutter(&mut app, &mut io, window_id, 2, ModifiersState::default());

    // The diff page's buffer is the third: file (0), status bar (1), diff.
    assert!(buffer_text(&app, 2).starts_with("Change:  qpvuntsmwlqt"));
    assert_eq!(io.open_windows.len(), 1);
    app.assert_invariants();
}

#[test]
fn ctrl_clicking_a_bar_opens_the_diff_page_in_a_new_window() {
    let (mut app, mut io, window_id) = file_app(TEXT, vec![range(2..3, VcsChangeKind::Modified)]);
    draw(&mut app, window_id);

    press_gutter(
        &mut app,
        &mut io,
        window_id,
        2,
        ModifiersState {
            control: true,
            ..ModifiersState::default()
        },
    );

    assert_eq!(io.open_windows.len(), 2);
    assert!(buffer_text(&app, 2).starts_with("Change:  qpvuntsmwlqt"));
    app.assert_invariants();
}

#[test]
fn clicking_the_gutter_away_from_a_bar_moves_the_cursor() {
    let (mut app, mut io, window_id) = file_app(TEXT, vec![range(2..3, VcsChangeKind::Modified)]);
    draw(&mut app, window_id);

    press_gutter(&mut app, &mut io, window_id, 3, ModifiersState::default());

    // No diff page: the click fell through to the editor, which put the
    // cursor on the line that was clicked.
    assert_eq!(app.buffers.keys().count(), 2);
    assert_eq!(buffer_text(&app, 1), "/repo/file.txt 4:1");
    app.assert_invariants();
}

#[test]
fn a_scrolled_view_marks_the_lines_it_is_showing() {
    let text: String = (0..40).map(|i| format!("line {i}\n")).collect();
    let (mut app, mut io, window_id) = file_app(&text, vec![range(20..22, VcsChangeKind::Added)]);
    draw(&mut app, window_id);
    // One wheel notch is 128 pixels, which is 8 rows of a 16 pixel cell.
    common::mouse_wheel(&mut app, &mut io, window_id, -2.0);
    common::tick(&mut app, &mut io);

    let drawing = draw(&mut app, window_id);

    // Lines 20 and 21, now that 16 rows have scrolled off the top.
    assert_eq!(
        gutter_bars(&app, &drawing),
        vec![(4, "added", 1.0), (5, "added", 1.0)]
    );
    app.assert_invariants();
}

#[test]
fn a_line_with_no_change_keeps_its_wrap_marker() {
    // The bars are drawn behind the gutter's own markers, not instead of
    // them: a wrapped, changed line shows both.
    let long = "x".repeat(WRAP_CHARS + 5);
    let text = format!("{long}\n");
    let (mut app, mut io, window_id) = file_app(&text, vec![range(0..1, VcsChangeKind::Added)]);
    common::tick(&mut app, &mut io);

    let drawing = draw(&mut app, window_id);

    let wrap_markers = drawing
        .commands
        .iter()
        .filter(|command| match command {
            DrawCommand::Character(character) => character.ch == '\\',
            _ => false,
        })
        .count();
    assert_eq!(wrap_markers, 1);
    assert_eq!(gutter_bars(&app, &drawing).len(), 2);
    app.assert_invariants();
}

#[test]
fn keys_still_reach_the_editor_with_a_bar_under_the_cursor() {
    let (mut app, mut io, window_id) = file_app(TEXT, vec![range(0..1, VcsChangeKind::Modified)]);
    draw(&mut app, window_id);

    common::key(&mut app, &mut io, window_id, Key::Character("z"));
    common::tick(&mut app, &mut io);

    assert_eq!(buffer_text(&app, 0), "zone\ntwo\nthree\nfour\nfive\n");
    app.assert_invariants();
}

// A file long enough that the scrollbar has to scale it down, with one
// changed line halfway through it.
const LONG_LINES: usize = 100;
const CHANGED_LINE: usize = 50;

fn long_file_app() -> (App, MockIO, WindowId) {
    let text: String = (0..LONG_LINES).map(|i| format!("line {i}\n")).collect();
    file_app(
        &text,
        vec![range(
            CHANGED_LINE..CHANGED_LINE + 1,
            VcsChangeKind::Modified,
        )],
    )
}

// The editor's viewport, which is the page minus its status bar row.
fn viewport(app: &App) -> [f32; 2] {
    let [cell_w, cell_h] = [app.cell_size()[0] as f32, app.cell_size()[1] as f32];
    [
        cell_w * (WRAP_CHARS + 2) as f32,
        cell_h * ROWS as f32 - cell_h - 1.0,
    ]
}

#[test]
fn the_scrollbar_shows_changes_that_are_scrolled_out_of_view() {
    let (mut app, mut io, window_id) = long_file_app();
    common::tick(&mut app, &mut io);

    let drawing = draw(&mut app, window_id);

    // The change is far below the viewport, so the left gutter shows
    // nothing and the scrollbar shows it anyway.
    assert_eq!(gutter_bars(&app, &drawing), vec![]);
    let marks = scrollbar_marks(&app, &drawing);
    assert_eq!(marks.len(), 1, "{marks:?}");
    let (y, kind, height) = marks[0];
    assert_eq!(kind, "modified");
    // Halfway down the file, so halfway down the scrollbar.
    let expected = viewport(&app)[1] * CHANGED_LINE as f32 / (LONG_LINES + 1) as f32;
    assert!((y - expected).abs() < 1.0, "{y} is not about {expected}");
    // One line of a hundred is less than a pixel; it is drawn anyway.
    assert_eq!(height, 2.0);
    app.assert_invariants();
}

#[test]
fn the_scrollbar_marks_sit_in_the_right_gutter() {
    let (mut app, mut io, window_id) = long_file_app();
    common::tick(&mut app, &mut io);

    let drawing = draw(&mut app, window_id);

    let cell_w = app.cell_size()[0] as f32;
    let marks: Vec<f32> = bars(&app, &drawing)
        .into_iter()
        .filter(|(x, ..)| *x > 0.0)
        .map(|(x, ..)| x)
        .collect();
    // The outer half of the last cell of the row.
    assert_eq!(marks, vec![viewport(&app)[0] - cell_w / 2.0]);
    app.assert_invariants();
}

#[test]
fn pressing_the_scrollbar_scrolls_there() {
    let (mut app, mut io, window_id) = long_file_app();
    draw(&mut app, window_id);
    let [viewport_w, viewport_h] = viewport(&app);

    // Halfway down the scrollbar is halfway down the file.
    let cell_w = app.cell_size()[0] as f32;
    let position = [viewport_w - cell_w / 2.0, viewport_h / 2.0];
    common::mouse_enter(&mut app, &mut io, window_id);
    common::mouse_moved(&mut app, &mut io, window_id, position);
    common::mouse_button(&mut app, &mut io, window_id, ButtonState::Pressed, position);
    common::mouse_button(
        &mut app,
        &mut io,
        window_id,
        ButtonState::Released,
        position,
    );
    common::tick(&mut app, &mut io);

    // The changed line is now on screen, so the left gutter marks it.
    let drawing = draw(&mut app, window_id);
    let gutter = gutter_bars(&app, &drawing);
    assert_eq!(gutter.len(), 1, "{gutter:?}");
    assert_eq!(gutter[0].1, "modified");
    // ... and the cursor stayed where it was, at the top of the file: a
    // press on the scrollbar scrolls, it does not move the cursor.
    assert_eq!(buffer_text(&app, 1), "/repo/file.txt 1:1");
    app.assert_invariants();
}
