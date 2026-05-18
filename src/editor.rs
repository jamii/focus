use std::time::Duration;

use bstr::{BStr, ByteSlice};
use winit::{
    event::ElementState,
    keyboard::{Key, NamedKey},
};

use crate::{
    app::{App, DocumentId, IO, InputEvent},
    document::{Edit, EditKind},
    style::{HIGHLIGHT_COLOR, TEXT_COLOR},
    text::{Drawing, Rect},
};

pub struct Editor {
    pub document_id: DocumentId,
    cursors: Vec<Cursor>,
    show_cursor: bool,
    wrap_chars: usize,
    wraps: Vec<[usize; 2]>,
    last_input: Duration,
}

struct Cursor {
    pos: usize,
}

enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl Editor {
    pub fn new(document_id: DocumentId) -> Self {
        Editor {
            document_id: document_id,
            cursors: vec![Cursor { pos: 0 }],
            show_cursor: true,
            wrap_chars: 0,
            wraps: vec![[0, 0]],
            last_input: Duration::ZERO,
        }
    }

    pub fn input(&mut self, app: &App, io: &mut dyn IO, event: InputEvent) {
        match event {
            InputEvent::KeyboardInput {
                event: key_event, ..
            } if key_event.state == ElementState::Pressed
                && app.modifiers.state().control_key()
                && !app.modifiers.state().alt_key() =>
            {
                match key_event.logical_key.as_ref() {
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
                    _ => {}
                }
            }
            InputEvent::KeyboardInput {
                event: key_event, ..
            } if key_event.state == ElementState::Pressed
                && !app.modifiers.state().control_key()
                && !app.modifiers.state().alt_key() =>
            {
                let mut document = self.document_id.get_mut(app);
                match key_event.logical_key.as_ref() {
                    Key::Character(char) => {
                        document.queue_edits(
                            self.cursors
                                .iter()
                                .map(|c| Edit {
                                    kind: EditKind::Insert,
                                    pos: c.pos,
                                    text: char.into(),
                                })
                                .collect(),
                        );
                    }
                    Key::Named(NamedKey::Enter) => {
                        document.queue_edits(
                            self.cursors
                                .iter()
                                .map(|c| Edit {
                                    kind: EditKind::Insert,
                                    pos: c.pos,
                                    text: '\n'.to_string().into(),
                                })
                                .collect(),
                        );
                    }
                    Key::Named(NamedKey::Space) => {
                        document.queue_edits(
                            self.cursors
                                .iter()
                                .map(|c| Edit {
                                    kind: EditKind::Insert,
                                    pos: c.pos,
                                    text: ' '.to_string().into(),
                                })
                                .collect(),
                        );
                    }
                    Key::Named(NamedKey::Backspace) => {
                        let mut edits = Vec::with_capacity(self.cursors.len());
                        for cursor in &self.cursors {
                            if let Some(start) = document.char_prev(cursor.pos) {
                                edits.push(Edit {
                                    kind: EditKind::Delete,
                                    pos: start,
                                    text: document.text[start..cursor.pos].into(),
                                })
                            }
                        }
                        document.queue_edits(edits);
                    }
                    Key::Named(NamedKey::Delete) => {
                        let mut edits = Vec::with_capacity(self.cursors.len());
                        for cursor in &self.cursors {
                            if let Some(end) = document.char_next(cursor.pos) {
                                edits.push(Edit {
                                    kind: EditKind::Delete,
                                    pos: cursor.pos,
                                    text: document.text[cursor.pos..end].into(),
                                })
                            }
                        }
                        document.queue_edits(edits);
                    }
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
        let wrap_chars = (drawing.current_clip_size()[0] / app.atlas.cell_size[0] as f32).floor() as isize
            // leave space for gutters
            - 2;
        if wrap_chars <= 0 {
            return;
        }
        if self.wrap_chars != wrap_chars as usize {
            self.wrap_chars = wrap_chars as usize;
            self.refresh_wraps(app);
        }

        {
            let text = &self.document_id.get(app).text;
            for [start, end] in &self.wraps {
                let mut grid = self.grid_from_pos(app, *start);
                grid[0] += 1; // gutter
                drawing.draw_text(
                    &app.atlas,
                    &text.as_bstr()[*start..*end],
                    app.atlas.screen_from_grid(grid),
                    TEXT_COLOR,
                );
                if *start > 0 && text[start - 1] != b'\n' {
                    drawing.draw_text(
                        &app.atlas,
                        BStr::new(b"\\"),
                        app.atlas.screen_from_grid([0, grid[1]]),
                        HIGHLIGHT_COLOR,
                    );
                }
            }
        }

        if self.show_cursor {
            for cursor in &self.cursors {
                let mut grid = self.grid_from_pos(app, cursor.pos);
                grid[0] += 1; // gutter
                let mut pos = app.atlas.screen_from_grid(grid);
                let mut size = [app.atlas.cell_size[0] as f32, app.atlas.cell_size[1] as f32];
                size[0] /= 8.0;
                pos[0] -= size[0] / 2.0;
                drawing.draw_rect(&app.atlas, Rect { pos, size }, TEXT_COLOR);
            }
        }
    }

    pub fn handle_edits(&mut self, app: &App, edits: &[Edit]) {
        // TODO This is quadratic - edits come from cursors. Should precompute a diff per range once and use for all editors.
        for cursor in &mut self.cursors {
            let mut insert_len = 0;
            let mut delete_len = 0;
            for edit in edits.iter() {
                match edit.kind {
                    EditKind::Insert => {
                        if cursor.pos >= edit.pos {
                            insert_len += edit.text.len();
                        }
                    }
                    EditKind::Delete => {
                        if cursor.pos > edit.pos {
                            delete_len += edit.text.len();
                        }
                    }
                }
            }
            cursor.pos += insert_len;
            cursor.pos -= delete_len;
        }
        self.refresh_wraps(app);
    }

    fn refresh_wraps(&mut self, app: &App) {
        let text = &self.document_id.get(app).text;
        self.wraps.clear();
        compute_wraps(text.as_bstr(), self.wrap_chars, &mut self.wraps);
    }

    fn cursor_move(&mut self, app: &App, direction: Direction) {
        let document = self.document_id.get(app);
        let pos_new = self
            .cursors
            .iter()
            .map(|cursor| match direction {
                Direction::Left => document.char_prev(cursor.pos),
                Direction::Right => document.char_next(cursor.pos),
                Direction::Up => self.line_up(app, cursor.pos),
                Direction::Down => self.line_down(app, cursor.pos),
            })
            .collect::<Vec<_>>();
        for (cursor, pos_new) in self.cursors.iter_mut().zip(pos_new.into_iter()) {
            if let Some(pos_new) = pos_new {
                cursor.pos = pos_new
            }
        }
    }

    fn grid_from_pos(&self, app: &App, pos: usize) -> [usize; 2] {
        let text = &self.document_id.get(app).text;
        grid_from_wraps(&self.wraps, text.as_bstr(), pos)
    }

    fn line_up(&self, app: &App, pos: usize) -> Option<usize> {
        let document = self.document_id.get(app);
        let line = self.grid_from_pos(app, pos)[1];
        if line == 0 {
            return None;
        }
        let col = document.text[self.wraps[line][0]..pos].chars().count();
        let wrap_prev = self.wraps[line - 1];
        let mut result_pos = wrap_prev[0];
        if let Some((_, char_end, _)) = document.text[wrap_prev[0]..wrap_prev[1]]
            .char_indices()
            .take(col)
            .last()
        {
            result_pos += char_end;
        }
        Some(result_pos)
    }

    fn line_down(&self, app: &App, pos: usize) -> Option<usize> {
        let document = self.document_id.get(app);
        let line = self.grid_from_pos(app, pos)[1];
        if line == self.wraps.len() - 1 {
            return None;
        }
        let col = document.text[self.wraps[line][0]..pos].chars().count();
        let wrap_next = self.wraps[line + 1];
        let mut result_pos = wrap_next[0];
        if let Some((_, char_end, _)) = document.text[wrap_next[0]..wrap_next[1]]
            .char_indices()
            .take(col)
            .last()
        {
            result_pos += char_end;
        }
        Some(result_pos)
    }
}

fn compute_wraps(text: &BStr, wrap_chars: usize, wraps: &mut Vec<[usize; 2]>) {
    let mut start = 0;
    let mut end = 0;
    let mut last_soft_wrap = None;
    let mut col = 0;
    while let Some((_, char_end, char)) = text[end..].char_indices().next() {
        if col >= wrap_chars {
            if let Some(pos) = last_soft_wrap {
                end = pos;
            }
            wraps.push([start, end]);
            start = end;
            col = 0;
            last_soft_wrap = None;
        }
        if char == '\n' {
            wraps.push([start, end]);
            start = end + char_end;
            col = 0;
            last_soft_wrap = None;
        } else {
            col += 1
        }
        end += char_end;
        if char == ' ' {
            last_soft_wrap = Some(end);
        }
    }
    wraps.push([start, text.len()]);
}

fn grid_from_wraps(wraps: &[[usize; 2]], text: &BStr, pos: usize) -> [usize; 2] {
    // TODO binary search
    for (line, [start, end]) in wraps.iter().enumerate().rev() {
        if *start <= pos && pos <= *end {
            let col = text[*start..pos].chars().count();
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
    fn grid_from_wraps_picks_later_line_on_boundary() {
        // pos sits on both the end of line 0 and the start of line 1 — the
        // reverse iteration returns the later (line 1) match.
        let text: BString = "ab\ncd".into();
        let wraps = vec![[0, 2], [3, 5]];
        assert_eq!(grid_from_wraps(&wraps, text.as_bstr(), 3), [0, 1]);
    }
}
