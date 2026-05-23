use std::ops::Range;
use std::time::Duration;

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
    head: usize,
    tail: usize,
}

enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl Editor {
    pub fn new(document_id: DocumentId, app: &App) -> Self {
        let mut editor = Editor {
            document_id: document_id,
            cursors: vec![Cursor { head: 0, tail: 0 }],
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
                    let grid = self.grid_from_offset(app, *start);
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
            let text = &self.document_id.get(app).text;
            for cursor in &self.cursors {
                if let Some(range) = cursor.marked_range(self.marked) {
                    for &[wrap_start, wrap_end] in &self.wraps {
                        if range.end <= wrap_start || range.start > wrap_end {
                            continue;
                        }
                        let mark_start = range.start.max(wrap_start);
                        let mark_end = range.end.min(wrap_end);
                        let grid_start = grid_from_wraps(&self.wraps, text.as_bstr(), mark_start);
                        let mut grid_end = grid_from_wraps(&self.wraps, text.as_bstr(), mark_end);
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
                let grid = self.grid_from_offset(app, *start);
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
                let grid_start = self.grid_from_offset(app, cursor.head);
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

    pub fn handle_edits(&mut self, app: &App, diff: &OffsetDiff) {
        for cursor in &mut self.cursors {
            cursor.head = diff.apply(cursor.head);
            cursor.tail = diff.apply(cursor.tail);
        }
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
            } else if let Some(start) = document.char_prev(cursor.head) {
                edits.push(Edit {
                    kind: EditKind::Delete,
                    offset: start,
                    text: document.text[start..cursor.head].into(),
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
            } else if let Some(end) = document.char_next(cursor.head) {
                edits.push(Edit {
                    kind: EditKind::Delete,
                    offset: cursor.head,
                    text: document.text[cursor.head..end].into(),
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
        let offsets_new = self
            .cursors
            .iter()
            .map(|cursor| match direction {
                Direction::Left => document.char_prev(cursor.head),
                Direction::Right => document.char_next(cursor.head),
                Direction::Up => self.line_up(app, cursor.head),
                Direction::Down => self.line_down(app, cursor.head),
            })
            .collect::<Vec<_>>();
        for (cursor, offset_new) in self.cursors.iter_mut().zip(offsets_new.into_iter()) {
            if let Some(offset_new) = offset_new {
                cursor.head = offset_new
            }
        }
    }

    fn grid_from_offset(&self, app: &App, offset: usize) -> [usize; 2] {
        let text = &self.document_id.get(app).text;
        grid_from_wraps(&self.wraps, text.as_bstr(), offset)
    }

    fn line_up(&self, app: &App, offset: usize) -> Option<usize> {
        let document = self.document_id.get(app);
        let line = self.grid_from_offset(app, offset)[1];
        if line == 0 {
            return None;
        }
        let col = document.text[self.wraps[line][0]..offset].chars().count();
        let wrap_prev = self.wraps[line - 1];
        let mut result_offset = wrap_prev[0];
        if let Some((_, char_end, _)) = document.text[wrap_prev[0]..wrap_prev[1]]
            .char_indices()
            .take(col)
            .last()
        {
            result_offset += char_end;
        }
        Some(result_offset)
    }

    fn line_down(&self, app: &App, offset: usize) -> Option<usize> {
        let document = self.document_id.get(app);
        let line = self.grid_from_offset(app, offset)[1];
        if line == self.wraps.len() - 1 {
            return None;
        }
        let col = document.text[self.wraps[line][0]..offset].chars().count();
        let wrap_next = self.wraps[line + 1];
        let mut result_offset = wrap_next[0];
        if let Some((_, char_end, _)) = document.text[wrap_next[0]..wrap_next[1]]
            .char_indices()
            .take(col)
            .last()
        {
            result_offset += char_end;
        }
        Some(result_offset)
    }
}

impl Cursor {
    fn marked_range(&self, marked: bool) -> Option<Range<usize>> {
        if marked && self.head != self.tail {
            Some(self.head.min(self.tail)..self.head.max(self.tail))
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
                offset: cursor.head,
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
            col += 1;
            end += char_end;
            if char == ' ' {
                last_soft_wrap = Some(end);
            }
            if col >= wrap_chars {
                if let Some(offset) = last_soft_wrap {
                    end = offset;
                }
                break;
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

fn grid_from_wraps(wraps: &[[usize; 2]], text: &BStr, offset: usize) -> [usize; 2] {
    // TODO binary search
    for (line, [start, end]) in wraps.iter().enumerate().rev() {
        if *start <= offset && offset <= *end {
            let col = text[*start..offset].chars().count();
            return [col, line];
        }
    }
    unreachable!();
}

#[cfg(test)]
mod tests {
    use super::*;
    use bstr::BString;

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
    fn grid_from_wraps_at_start() {
        let text: BString = "hello\nworld".into();
        let wraps = vec![[0, 5], [6, 11]];
        assert_eq!(grid_from_wraps(&wraps, text.as_bstr(), 0), [0, 0]);
    }

    #[test]
    fn grid_from_wraps_at_end_of_first_line() {
        let text: BString = "hello\nworld".into();
        let wraps = vec![[0, 5], [6, 11]];
        assert_eq!(grid_from_wraps(&wraps, text.as_bstr(), 5), [5, 0]);
    }

    #[test]
    fn grid_from_wraps_at_start_of_second_line() {
        let text: BString = "hello\nworld".into();
        let wraps = vec![[0, 5], [6, 11]];
        assert_eq!(grid_from_wraps(&wraps, text.as_bstr(), 6), [0, 1]);
    }

    #[test]
    fn grid_from_wraps_at_end_of_text() {
        let text: BString = "hello\nworld".into();
        let wraps = vec![[0, 5], [6, 11]];
        assert_eq!(grid_from_wraps(&wraps, text.as_bstr(), 11), [5, 1]);
    }

    #[test]
    fn grid_from_wraps_counts_chars_not_bytes() {
        // 'é' is 2 bytes but 1 column.
        let text: BString = "héllo".into();
        let wraps = vec![[0, 6]];
        assert_eq!(grid_from_wraps(&wraps, text.as_bstr(), 3), [2, 0]);
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

    #[test]
    fn grid_from_wraps_picks_later_line_on_boundary() {
        // offset sits on both the end of line 0 and the start of line 1 — the
        // reverse iteration returns the later (line 1) match.
        let text: BString = "ab\ncd".into();
        let wraps = vec![[0, 2], [3, 5]];
        assert_eq!(grid_from_wraps(&wraps, text.as_bstr(), 3), [0, 1]);
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
        let c = Cursor { head: 7, tail: 3 };
        assert_eq!(c.marked_range(false), None);
    }

    #[test]
    fn marked_range_orders_head_and_tail() {
        let c = Cursor { head: 7, tail: 3 };
        assert_eq!(c.marked_range(true), Some(3..7));
        let c = Cursor { head: 3, tail: 7 };
        assert_eq!(c.marked_range(true), Some(3..7));
    }

    #[test]
    fn marked_range_with_equal_head_and_tail_is_none() {
        let c = Cursor { head: 4, tail: 4 };
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
        let cursors = vec![Cursor { head: 3, tail: 0 }];
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
        let cursors = vec![Cursor { head: 4, tail: 1 }];
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
        let cursors = vec![Cursor { head: 2, tail: 2 }];
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
        let mut cursors = vec![Cursor { head: 4, tail: 1 }];
        let edits = calculate_replace_edits(&cursors, true, &d, b"X");
        let diff = d.apply_edits(&edits);
        for c in &mut cursors {
            c.head = diff.apply(c.head);
            c.tail = diff.apply(c.tail);
        }
        assert_eq!(d.text, "hXo");
        assert_eq!(cursors[0].head, 2);
        assert_eq!(cursors[0].tail, 2);
    }

    #[test]
    fn calculate_replace_edits_apply_inserts_when_selection_is_empty() {
        // No mark → single insert; both head and tail shift past it.
        let mut d = doc_with("hello");
        let mut cursors = vec![Cursor { head: 2, tail: 2 }];
        let edits = calculate_replace_edits(&cursors, false, &d, b"X");
        let diff = d.apply_edits(&edits);
        for c in &mut cursors {
            c.head = diff.apply(c.head);
            c.tail = diff.apply(c.tail);
        }
        assert_eq!(d.text, "heXllo");
        assert_eq!(cursors[0].head, 3);
        assert_eq!(cursors[0].tail, 3);
    }

    #[test]
    fn calculate_replace_edits_apply_handles_head_before_tail() {
        // Head < tail: selection still spans [min, max], result is the same.
        let mut d = doc_with("hello");
        let mut cursors = vec![Cursor { head: 1, tail: 4 }];
        let edits = calculate_replace_edits(&cursors, true, &d, b"X");
        let diff = d.apply_edits(&edits);
        for c in &mut cursors {
            c.head = diff.apply(c.head);
            c.tail = diff.apply(c.tail);
        }
        assert_eq!(d.text, "hXo");
        assert_eq!(cursors[0].head, 2);
        assert_eq!(cursors[0].tail, 2);
    }
}
