use std::time::Duration;
use std::{mem::replace, ops::Range};

use bstr::{BStr, ByteSlice};
use winit::{
    event::ElementState,
    keyboard::{Key, NamedKey},
};

use crate::{
    app::{App, DocumentId, IO, InputEvent},
    document::{Document, Edit, EditKind, OffsetDiff},
    drawing::{Drawing, Rect},
    style::{HIGHLIGHT_COLOR, TEXT_COLOR},
};

pub struct Editor {
    pub document_id: DocumentId,
    cursors: Vec<Cursor>,
    marked: bool,
    show_cursor: bool,
    wrap_chars: usize,
    wraps: Vec<[usize; 2]>,
    last_input: Duration,
}

struct Cursor {
    head: CursorPoint,
    tail: CursorPoint,
}

#[derive(Copy, Clone)]
struct CursorPoint {
    offset: usize,
    // The column the cursor 'wants' to be at when moving up/down, if any.
    col_wanted: Option<usize>,
}

enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl Editor {
    pub fn assert_invariants(&self, app: &App) {
        let document = self.document_id.get(app);
        for cursor in &self.cursors {
            for point in [&cursor.head, &cursor.tail] {
                assert!(point.offset <= document.text.len());
            }
        }

        assert!(!self.wraps.is_empty());
        assert_eq!(self.wraps[0][0], 0);
        assert_eq!(self.wraps.last().unwrap()[1], document.text.len());
        for wrap in &self.wraps {
            assert!(wrap[0] <= wrap[1]);
            assert!(document.text[wrap[0]..wrap[1]].chars().count() <= self.wrap_chars)
        }
        for pair in self.wraps.windows(2) {
            let gap = pair[1][0] - pair[0][1];
            assert!(gap <= 1);
            if gap == 1 {
                assert!(document.text[pair[0][1]..].chars().next().unwrap() == '\n')
            }
        }
    }

    pub fn new(document_id: DocumentId, app: &App) -> Self {
        let mut editor = Editor {
            document_id: document_id,
            cursors: vec![Cursor {
                head: CursorPoint {
                    offset: 0,
                    col_wanted: None,
                },
                tail: CursorPoint {
                    offset: 0,
                    col_wanted: None,
                },
            }],
            marked: false,
            show_cursor: true,
            wrap_chars: 80,
            wraps: vec![],
            last_input: Duration::ZERO,
        };
        editor.refresh_wraps(app);
        editor
    }

    pub fn input(&mut self, app: &App, io: &mut dyn IO, event: InputEvent) {
        match event {
            InputEvent::Key {
                state, logical_key, ..
            } if state == ElementState::Pressed
                && app.modifiers.control_key()
                && !app.modifiers.alt_key() =>
            {
                match logical_key.as_ref() {
                    Key::Character("i") => {
                        self.cursor_move(app, Direction::Up);
                    }
                    Key::Character("k") => {
                        self.cursor_move(app, Direction::Down);
                    }
                    Key::Character("j") => {
                        self.cursor_move(app, Direction::Left);
                    }
                    Key::Character("l") => {
                        self.cursor_move(app, Direction::Right);
                    }
                    Key::Named(NamedKey::Space) => {
                        self.toggle_mark();
                    }
                    _ => {}
                }
            }
            InputEvent::Key {
                state, logical_key, ..
            } if state == ElementState::Pressed
                && !app.modifiers.control_key()
                && !app.modifiers.alt_key() =>
            {
                match logical_key.as_ref() {
                    Key::Character(char) => self.cursor_replace(app, char.as_bytes()),
                    Key::Named(NamedKey::Enter) => self.cursor_replace(app, b"\n"),
                    Key::Named(NamedKey::Space) => self.cursor_replace(app, b" "),
                    Key::Named(NamedKey::Backspace) => self.cursor_delete_left(app),
                    Key::Named(NamedKey::Delete) => self.cursor_delete_right(app),
                    _ => {}
                }
            }
            _ => {}
        }
        self.last_input = io.frame_start();
    }

    pub fn tick(&mut self, _app: &App, io: &mut dyn IO, redraw: &mut bool) {
        let show_cursor = ((io.frame_start().as_millis() / 500) % 2) == 0
            || (io.frame_start() - self.last_input < Duration::from_millis(500));
        if self.show_cursor != show_cursor {
            self.show_cursor = show_cursor;
            *redraw = true;
        }
    }

    pub fn draw(&mut self, app: &App, drawing: &mut Drawing) {
        // Update wrapping.
        let clip_size = drawing.size();
        let wrap_chars = app.atlas.grid_from_screen(clip_size)[0] as usize;
        if wrap_chars <= 2 {
            return;
        }
        // Leave space for gutters
        let wrap_chars = wrap_chars - 2;
        if self.wrap_chars != wrap_chars {
            self.wrap_chars = wrap_chars;
            self.refresh_wraps(app);
        }

        // Draw gutters.
        {
            let text = &self.document_id.get(app).text;
            for [start, _end] in &self.wraps {
                if *start > 0 && text[start - 1] != b'\n' {
                    let grid = self.grid_from_offset(app, *start)[1];
                    drawing.draw_text(
                        &app.atlas,
                        BStr::new(b"\\"),
                        app.atlas.screen_from_grid([0, grid[1]]),
                        HIGHLIGHT_COLOR,
                    );
                }
            }
        }

        // Clip the gutters out.
        let width = app.atlas.screen_from_grid([wrap_chars, 0])[0];
        let mut drawing = drawing.push_clip_rect(Rect {
            pos: app.atlas.screen_from_grid([1, 0]),
            size: [width, clip_size[1]],
        });

        // Draw mark.
        if self.marked {
            for cursor in &self.cursors {
                if let Some(range) = cursor.marked_range(self.marked) {
                    for &[wrap_start, wrap_end] in &self.wraps {
                        if range.end <= wrap_start || range.start > wrap_end {
                            continue;
                        }
                        let mark_start = range.start.max(wrap_start);
                        let mark_end = range.end.min(wrap_end);
                        let grid_start = self.grid_from_offset(app, mark_start)[1];
                        let mut grid_end = self.grid_from_offset(app, mark_end)[0];
                        grid_end[1] += 1;
                        let screen_start = app.atlas.screen_from_grid(grid_start);
                        let screen_end = app.atlas.screen_from_grid(grid_end);
                        drawing.draw_rect(
                            &app.atlas,
                            Rect::from_corners(screen_start, screen_end),
                            HIGHLIGHT_COLOR,
                        );
                    }
                }
            }
        }

        // Draw text.
        {
            let text = &self.document_id.get(app).text;
            for [start, end] in &self.wraps {
                let grid = self.grid_from_offset(app, *start)[1];
                let screen = app.atlas.screen_from_grid(grid);
                drawing.draw_text(
                    &app.atlas,
                    &text.as_bstr()[*start..*end],
                    screen,
                    TEXT_COLOR,
                );
            }
        }

        // Draw cursors.
        if self.show_cursor {
            for cursor in &self.cursors {
                for grid_start in self.grid_from_offset(app, cursor.head.offset) {
                    let mut grid_end = grid_start;
                    grid_end[1] += 1;
                    let mut screen_start = app.atlas.screen_from_grid(grid_start);
                    let mut screen_end = app.atlas.screen_from_grid(grid_end);
                    let w = app.atlas.cell_size[0] as f32 / 8.0;
                    screen_start[0] -= w / 2.0;
                    screen_end[0] += w / 2.0;
                    drawing.draw_rect(
                        &app.atlas,
                        Rect::from_corners(screen_start, screen_end),
                        TEXT_COLOR,
                    );
                }
            }
        }
    }

    pub fn handle_edits(&mut self, app: &App, diff: &OffsetDiff) {
        let mut cursors = replace(&mut self.cursors, vec![]);
        for cursor in &mut cursors {
            for point in [&mut cursor.head, &mut cursor.tail] {
                *point = CursorPoint {
                    offset: diff.apply(point.offset),
                    col_wanted: None,
                };
            }
        }
        self.cursors = cursors;
        self.refresh_wraps(app);
    }

    fn toggle_mark(&mut self) {
        if self.marked {
            self.marked = false;
        } else {
            self.marked = true;
            for cursor in &mut self.cursors {
                cursor.tail = cursor.head;
            }
        }
    }

    fn cursor_replace(&mut self, app: &App, insert: &[u8]) {
        let mut document = self.document_id.get_mut(app);
        let edits = calculate_replace_edits(&self.cursors, self.marked, &document, insert);
        document.queue_edits(edits);
        self.marked = false;
    }

    fn cursor_delete_left(&mut self, app: &App) {
        let mut document = self.document_id.get_mut(app);
        let mut edits = Vec::with_capacity(self.cursors.len());
        for cursor in &self.cursors {
            if let Some(range) = cursor.marked_range(self.marked) {
                edits.push(Edit {
                    kind: EditKind::Delete,
                    offset: range.start,
                    text: document.text[range.start..range.end].into(),
                });
            } else if let Some(start) = document.char_prev(cursor.head.offset) {
                edits.push(Edit {
                    kind: EditKind::Delete,
                    offset: start,
                    text: document.text[start..cursor.head.offset].into(),
                });
            }
        }
        document.queue_edits(edits);
        self.marked = false;
    }

    fn cursor_delete_right(&mut self, app: &App) {
        let mut document = self.document_id.get_mut(app);
        let mut edits = Vec::with_capacity(self.cursors.len());
        for cursor in &self.cursors {
            if let Some(range) = cursor.marked_range(self.marked) {
                edits.push(Edit {
                    kind: EditKind::Delete,
                    offset: range.start,
                    text: document.text[range.start..range.end].into(),
                });
            } else if let Some(end) = document.char_next(cursor.head.offset) {
                edits.push(Edit {
                    kind: EditKind::Delete,
                    offset: cursor.head.offset,
                    text: document.text[cursor.head.offset..end].into(),
                });
            }
        }
        document.queue_edits(edits);
        self.marked = false;
    }

    fn refresh_wraps(&mut self, app: &App) {
        let text = &self.document_id.get(app).text;
        self.wraps.clear();
        compute_wraps(text.as_bstr(), self.wrap_chars, &mut self.wraps);
    }

    fn cursor_move(&mut self, app: &App, direction: Direction) {
        let document = self.document_id.get(app);
        let mut cursors = replace(&mut self.cursors, vec![]);
        for cursor in &mut cursors {
            match direction {
                Direction::Left => {
                    cursor.head = CursorPoint {
                        offset: document
                            .char_prev(cursor.head.offset)
                            .unwrap_or(cursor.head.offset),
                        col_wanted: None,
                    };
                }
                Direction::Right => {
                    cursor.head = CursorPoint {
                        offset: document
                            .char_next(cursor.head.offset)
                            .unwrap_or(cursor.head.offset),
                        col_wanted: None,
                    };
                }
                Direction::Up => {
                    cursor.head = self.line_up(app, cursor.head).unwrap_or(cursor.head);
                }
                Direction::Down => {
                    cursor.head = self.line_down(app, cursor.head).unwrap_or(cursor.head);
                }
            };
        }
        self.cursors = cursors;
    }

    // Return the grid position for a byte offset within the doc.
    // When the cursor is at a soft-wrap position there are two possible grid positions:
    // * at the end of one soft-wrapped line
    // * at the start of the next soft-wrapped line
    // This returns both.
    // If the position is not ambiguous then both returned positions are equal.
    fn grid_from_offset(&self, app: &App, offset: usize) -> [[usize; 2]; 2] {
        let text = &self.document_id.get(app).text;
        let line = self.wraps.partition_point(|&[start, _end]| start <= offset) - 1;
        let grid1 = {
            let [start, end] = self.wraps[line];
            assert!(offset <= end);
            let col = text[start..offset].chars().count();
            [col, line]
        };
        let grid0 = if line > 0 && self.wraps[line - 1][1] == offset {
            let [start, end] = self.wraps[line - 1];
            assert!(offset == end);
            let col = text[start..offset].chars().count();
            [col, line - 1]
        } else {
            grid1
        };
        [grid0, grid1]
    }

    fn line_up(&self, app: &App, point: CursorPoint) -> Option<CursorPoint> {
        let document = self.document_id.get(app);
        let line = self.grid_from_offset(app, point.offset)[0][1];
        if line == 0 {
            return None;
        }
        let col = point.col_wanted.unwrap_or(
            document.text[self.wraps[line][0]..point.offset]
                .chars()
                .count(),
        );
        let wrap_prev = self.wraps[line - 1];
        let mut result_offset = wrap_prev[0];
        if let Some((_, char_end, _)) = document.text[wrap_prev[0]..wrap_prev[1]]
            .char_indices()
            .take(col)
            .last()
        {
            result_offset += char_end;
        }
        Some(CursorPoint {
            offset: result_offset,
            col_wanted: Some(col),
        })
    }

    fn line_down(&self, app: &App, point: CursorPoint) -> Option<CursorPoint> {
        let document = self.document_id.get(app);
        let line = self.grid_from_offset(app, point.offset)[0][1];
        if line == self.wraps.len() - 1 {
            return None;
        }
        let col = point.col_wanted.unwrap_or(
            document.text[self.wraps[line][0]..point.offset]
                .chars()
                .count(),
        );
        let wrap_next = self.wraps[line + 1];
        let mut result_offset = wrap_next[0];
        if let Some((_, char_end, _)) = document.text[wrap_next[0]..wrap_next[1]]
            .char_indices()
            .take(col)
            .last()
        {
            result_offset += char_end;
        }
        Some(CursorPoint {
            offset: result_offset,
            col_wanted: Some(col),
        })
    }
}

impl Cursor {
    fn marked_range(&self, marked: bool) -> Option<Range<usize>> {
        if marked && self.head.offset != self.tail.offset {
            Some(self.head.offset.min(self.tail.offset)..self.head.offset.max(self.tail.offset))
        } else {
            None
        }
    }
}

fn calculate_replace_edits(
    cursors: &[Cursor],
    marked: bool,
    document: &Document,
    insert: &[u8],
) -> Vec<Edit> {
    let mut edits = Vec::with_capacity(cursors.len() * 2);
    for cursor in cursors {
        if let Some(range) = cursor.marked_range(marked) {
            edits.push(Edit {
                kind: EditKind::Insert,
                offset: range.start,
                text: insert.into(),
            });
            edits.push(Edit {
                kind: EditKind::Delete,
                offset: range.start,
                text: document.text[range.start..range.end].into(),
            });
        } else {
            edits.push(Edit {
                kind: EditKind::Insert,
                offset: cursor.head.offset,
                text: insert.into(),
            });
        }
    }
    edits
}

fn compute_wraps(text: &BStr, wrap_chars: usize, wraps: &mut Vec<[usize; 2]>) {
    assert!(wrap_chars > 0);
    let mut end = 0;
    loop {
        let start = end;
        let mut col = 0;
        let mut last_soft_wrap = None;
        let mut newline = false;
        while let Some((_, char_end, char)) = text[end..].char_indices().next() {
            if char == '\n' {
                newline = true;
                break;
            }
            if col >= wrap_chars {
                if let Some(offset) = last_soft_wrap {
                    end = offset;
                }
                break;
            }
            col += 1;
            end += char_end;
            if char == ' ' {
                last_soft_wrap = Some(end);
            }
        }
        wraps.push([start, end]);
        if end >= text.len() {
            break;
        }
        if newline {
            end += 1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wraps_of(s: &str, wrap_chars: usize) -> Vec<[usize; 2]> {
        let mut wraps = vec![];
        compute_wraps(s.as_bytes().as_bstr(), wrap_chars, &mut wraps);
        wraps
    }

    #[test]
    fn empty_text_produces_one_empty_wrap() {
        assert_eq!(wraps_of("", 10), vec![[0, 0]]);
    }

    #[test]
    fn short_line_is_one_wrap() {
        assert_eq!(wraps_of("abc", 10), vec![[0, 3]]);
    }

    #[test]
    fn newline_splits_into_two_wraps() {
        assert_eq!(wraps_of("ab\ncd", 10), vec![[0, 2], [3, 5]]);
    }

    #[test]
    fn lone_newline_produces_two_empty_wraps() {
        assert_eq!(wraps_of("\n", 10), vec![[0, 0], [1, 1]]);
    }

    #[test]
    fn trailing_newline_leaves_empty_final_wrap() {
        assert_eq!(wraps_of("ab\n", 10), vec![[0, 2], [3, 3]]);
    }

    #[test]
    fn hard_wrap_when_no_space_available() {
        assert_eq!(wraps_of("abcde", 3), vec![[0, 3], [3, 5]]);
    }

    #[test]
    fn soft_wrap_breaks_after_last_space() {
        // "ab cdef" with width 4: 'd' would overflow, but we have a space at
        // byte 3, so the first wrap ends right after it.
        assert_eq!(wraps_of("ab cdef", 4), vec![[0, 3], [3, 7]]);
    }

    #[test]
    fn soft_wrap_does_not_reuse_earlier_space_after_overflow() {
        // After the soft wrap "ab " / "cdef", "cdef" has no space — the next
        // overflow must hard-wrap, not jump back to the previous space.
        assert_eq!(wraps_of("ab cdefgh", 4), vec![[0, 3], [3, 7], [7, 9]]);
    }

    #[test]
    fn multi_byte_chars_count_bytes_in_wrap_ranges() {
        // "héllo" is 6 bytes ('é' is 2), 5 chars — fits in width 10.
        assert_eq!(wraps_of("héllo", 10), vec![[0, 6]]);
    }

    #[test]
    fn soft_wrap_before_multi_byte_char_keeps_it_intact() {
        // "a b éef" — chars: a, ' ', b, ' ', é (2 bytes), e, f (8 bytes total).
        // With wrap_chars=3, "a " soft-wraps before 'b', then "b " soft-wraps
        // before 'é', then "éef" fits exactly. The third wrap must contain
        // the entire 'é' char, not split its bytes between wraps.
        assert_eq!(wraps_of("a b éef", 3), vec![[0, 2], [2, 4], [4, 8]]);
    }

    fn apply_helper(offset: usize, edits: &[Edit]) -> usize {
        let max_edit_end = edits
            .iter()
            .map(|e| match e.kind {
                EditKind::Insert => e.offset,
                EditKind::Delete => e.offset + e.text.len(),
            })
            .max()
            .unwrap_or(0);
        let old_len = offset.max(max_edit_end);
        let diff = OffsetDiff::from_edits(edits, old_len);
        diff.apply(offset)
    }

    #[test]
    fn shift_cursor_through_earlier_delete() {
        let edits = vec![Edit {
            kind: EditKind::Delete,
            offset: 3,
            text: "a".into(),
        }];
        assert_eq!(apply_helper(10, &edits), 9);
    }

    #[test]
    fn shift_cursor_unaffected_by_later_edits() {
        let edits = vec![
            Edit {
                kind: EditKind::Insert,
                offset: 10,
                text: "X".into(),
            },
            Edit {
                kind: EditKind::Delete,
                offset: 11,
                text: "y".into(),
            },
        ];
        assert_eq!(apply_helper(5, &edits), 5);
    }

    #[test]
    fn shift_cursor_inside_delete_range_clamps_to_delete_start() {
        let edits = vec![Edit {
            kind: EditKind::Delete,
            offset: 3,
            text: "abcd".into(),
        }];
        // Cursor at offset 5 is inside the deleted range [3..7); after the
        // edit it should land at 3 (the start of the deletion), not at 1.
        assert_eq!(apply_helper(5, &edits), 3);
    }

    fn doc_with(s: &str) -> Document {
        let mut d = Document::new();
        d.apply_edits(&[Edit {
            kind: EditKind::Insert,
            offset: 0,
            text: s.into(),
        }]);
        d
    }

    #[test]
    fn marked_range_unmarked_is_none() {
        let c = Cursor {
            head: CursorPoint {
                offset: 7,
                col_wanted: None,
            },
            tail: CursorPoint {
                offset: 3,
                col_wanted: None,
            },
        };
        assert_eq!(c.marked_range(false), None);
    }

    #[test]
    fn marked_range_orders_head_and_tail() {
        let c = Cursor {
            head: CursorPoint {
                offset: 7,
                col_wanted: None,
            },
            tail: CursorPoint {
                offset: 3,
                col_wanted: None,
            },
        };
        assert_eq!(c.marked_range(true), Some(3..7));
        let c = Cursor {
            head: CursorPoint {
                offset: 3,
                col_wanted: None,
            },
            tail: CursorPoint {
                offset: 7,
                col_wanted: None,
            },
        };
        assert_eq!(c.marked_range(true), Some(3..7));
    }

    #[test]
    fn marked_range_with_equal_head_and_tail_is_none() {
        let c = Cursor {
            head: CursorPoint {
                offset: 4,
                col_wanted: None,
            },
            tail: CursorPoint {
                offset: 4,
                col_wanted: None,
            },
        };
        assert_eq!(c.marked_range(true), None);
    }

    #[test]
    fn handle_edits_shifts_tail_through_earlier_insert() {
        let edits = [Edit {
            kind: EditKind::Insert,
            offset: 2,
            text: "abc".into(),
        }];
        assert_eq!(apply_helper(10, &edits), 13);
        assert_eq!(apply_helper(6, &edits), 9);
    }

    #[test]
    fn handle_edits_collapses_selection_inside_delete_range() {
        let edits = [Edit {
            kind: EditKind::Delete,
            offset: 3,
            text: "abcdef".into(),
        }];
        assert_eq!(apply_helper(7, &edits), 3);
        assert_eq!(apply_helper(5, &edits), 3);
    }

    #[test]
    fn calculate_replace_edits_unmarked_emits_single_insert_at_offset() {
        let d = doc_with("hello");
        let cursors = vec![Cursor {
            head: CursorPoint {
                offset: 3,
                col_wanted: None,
            },
            tail: CursorPoint {
                offset: 0,
                col_wanted: None,
            },
        }];
        let edits = calculate_replace_edits(&cursors, false, &d, b"X");
        assert_eq!(edits.len(), 1);
        assert!(matches!(edits[0].kind, EditKind::Insert));
        assert_eq!(edits[0].offset, 3);
        assert_eq!(edits[0].text, "X");
    }

    #[test]
    fn calculate_replace_edits_marked_with_selection_emits_insert_then_delete_at_sel_start() {
        let d = doc_with("hello");
        // Selection covers "ell" (positions 1..4).
        let cursors = vec![Cursor {
            head: CursorPoint {
                offset: 4,
                col_wanted: None,
            },
            tail: CursorPoint {
                offset: 1,
                col_wanted: None,
            },
        }];
        let edits = calculate_replace_edits(&cursors, true, &d, b"X");
        assert_eq!(edits.len(), 2);
        assert!(matches!(edits[0].kind, EditKind::Insert));
        assert_eq!(edits[0].offset, 1);
        assert_eq!(edits[0].text, "X");
        assert!(matches!(edits[1].kind, EditKind::Delete));
        assert_eq!(edits[1].offset, 1);
        assert_eq!(edits[1].text, "ell");
    }

    #[test]
    fn calculate_replace_edits_marked_with_empty_selection_emits_just_insert() {
        let d = doc_with("hello");
        let cursors = vec![Cursor {
            head: CursorPoint {
                offset: 2,
                col_wanted: None,
            },
            tail: CursorPoint {
                offset: 2,
                col_wanted: None,
            },
        }];
        let edits = calculate_replace_edits(&cursors, true, &d, b"X");
        assert_eq!(edits.len(), 1);
        assert!(matches!(edits[0].kind, EditKind::Insert));
        assert_eq!(edits[0].offset, 2);
    }

    #[test]
    fn calculate_replace_edits_apply_replaces_selection_in_document() {
        // End-to-end: the emitted edits, when applied, produce "hXo" from
        // "hello" and leave both head and tail at the position after "X".
        let mut d = doc_with("hello");
        let mut cursors = vec![Cursor {
            head: CursorPoint {
                offset: 4,
                col_wanted: None,
            },
            tail: CursorPoint {
                offset: 1,
                col_wanted: None,
            },
        }];
        let edits = calculate_replace_edits(&cursors, true, &d, b"X");
        let diff = d.apply_edits(&edits);
        for c in &mut cursors {
            c.head.offset = diff.apply(c.head.offset);
            c.tail.offset = diff.apply(c.tail.offset);
        }
        assert_eq!(d.text, "hXo");
        assert_eq!(cursors[0].head.offset, 2);
        assert_eq!(cursors[0].tail.offset, 2);
    }

    #[test]
    fn calculate_replace_edits_apply_inserts_when_selection_is_empty() {
        // No mark → single insert; both head and tail shift past it.
        let mut d = doc_with("hello");
        let mut cursors = vec![Cursor {
            head: CursorPoint {
                offset: 2,
                col_wanted: None,
            },
            tail: CursorPoint {
                offset: 2,
                col_wanted: None,
            },
        }];
        let edits = calculate_replace_edits(&cursors, false, &d, b"X");
        let diff = d.apply_edits(&edits);
        for c in &mut cursors {
            c.head.offset = diff.apply(c.head.offset);
            c.tail.offset = diff.apply(c.tail.offset);
        }
        assert_eq!(d.text, "heXllo");
        assert_eq!(cursors[0].head.offset, 3);
        assert_eq!(cursors[0].tail.offset, 3);
    }

    #[test]
    fn calculate_replace_edits_apply_handles_head_before_tail() {
        // Head < tail: selection still spans [min, max], result is the same.
        let mut d = doc_with("hello");
        let mut cursors = vec![Cursor {
            head: CursorPoint {
                offset: 1,
                col_wanted: None,
            },
            tail: CursorPoint {
                offset: 4,
                col_wanted: None,
            },
        }];
        let edits = calculate_replace_edits(&cursors, true, &d, b"X");
        let diff = d.apply_edits(&edits);
        for c in &mut cursors {
            c.head.offset = diff.apply(c.head.offset);
            c.tail.offset = diff.apply(c.tail.offset);
        }
        assert_eq!(d.text, "hXo");
        assert_eq!(cursors[0].head.offset, 2);
        assert_eq!(cursors[0].tail.offset, 2);
    }
}
