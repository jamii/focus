use std::time::Duration;
use std::{mem::replace, ops::Range};

use bstr::{BStr, ByteSlice};
use winit::{
    event::ElementState,
    keyboard::{Key, NamedKey},
};

use crate::style::BACKGROUND_COLOR;
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
    top_pixel: isize,
    last_viewport_size: [f32; 2],
    scroll_to_main_cursor: bool,
    dragging: Option<DragInfo>,
}

const SCROLL_AMOUNT: f32 = 32.0;

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

#[derive(Copy, Clone)]
struct DragInfo {
    cursor_index: usize,
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
            top_pixel: 0,
            last_viewport_size: [0.0, 0.0],
            scroll_to_main_cursor: false,
            dragging: None,
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
                && app.modifiers.alt_key() =>
            {
                match logical_key.as_ref() {
                    Key::Character("j") => {
                        self.cursor_goto_line_start(app);
                    }
                    Key::Character("l") => {
                        self.cursor_goto_line_end(app);
                    }
                    Key::Character("i") => {
                        self.cursor_goto_doc_start(app);
                    }
                    Key::Character("k") => {
                        self.cursor_goto_doc_end(app);
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
            InputEvent::MouseButton { state, position } => {
                match state {
                    ElementState::Pressed => {
                        let offset = self.offset_from_screen(app, position);

                        // Ctrl-click / Ctrl-drag: add a new cursor
                        // Click / drag: set main cursor, remove others
                        if !app.modifiers.control_key() {
                            self.cursors.clear();
                        }

                        let idx = self.cursors.len();
                        self.cursors.push(Cursor {
                            head: CursorPoint {
                                offset,
                                col_wanted: None,
                            },
                            tail: CursorPoint {
                                offset,
                                col_wanted: None,
                            },
                        });
                        self.dragging = Some(DragInfo { cursor_index: idx });
                        self.marked = false;
                        self.scroll_to_main_cursor = true;
                    }
                    ElementState::Released => {
                        self.dragging = None;
                    }
                }
            }
            InputEvent::MouseWheel { y_offset } => {
                self.top_pixel -= (SCROLL_AMOUNT * y_offset) as isize;
            }
            _ => {}
        }
        self.last_input = io.frame_start();
    }

    pub fn tick(&mut self, app: &App, io: &mut dyn IO, redraw: &mut bool) {
        // During drag, poll mouse position and update cursor head.
        if let Some(drag_info) = self.dragging {
            let mouse_pos = io.mouse_position();
            // Scroll when mouse is off-screen vertically.
            if mouse_pos[1] < 0.0 {
                self.top_pixel -= SCROLL_AMOUNT as isize;
            } else if mouse_pos[1] > self.last_viewport_size[1] {
                self.top_pixel += SCROLL_AMOUNT as isize;
            }
            self.clamp_top_pixel(app);

            // Convert screen position to document offset, accounting for scroll.
            let offset = self.offset_from_screen(app, mouse_pos);
            if let Some(cursor) = self.cursors.get_mut(drag_info.cursor_index) {
                if cursor.head.offset != offset {
                    self.marked = true;
                }
                cursor.head = CursorPoint {
                    offset,
                    col_wanted: None,
                };
            }

            self.last_input = io.frame_start();
            *redraw = true;
        }

        let show_cursor = ((io.frame_start().as_millis() / 500) % 2) == 0
            || (io.frame_start() - self.last_input < Duration::from_millis(500));
        if self.show_cursor != show_cursor {
            self.show_cursor = show_cursor;
            *redraw = true;
        }
    }

    pub fn draw(&mut self, app: &App, drawing: &mut Drawing) {
        // Update wrapping.
        let viewport_size = drawing.size();
        let grid_w = app.atlas.grid_from_screen(viewport_size)[0] as usize;
        if grid_w <= 2 {
            return;
        }
        // Leave space for gutters
        let wrap_chars = grid_w - 2;

        let center_before = self.center_offset(app);

        if self.wrap_chars != wrap_chars {
            self.wrap_chars = wrap_chars;
            self.refresh_wraps(app);
        }

        let viewport_changed = self.last_viewport_size != viewport_size;
        self.last_viewport_size = viewport_size;

        // Handle viewport resize: re-center on the same logical position.
        if viewport_changed {
            let center_before = center_before.min(self.document_id.get(app).text.len());
            self.scroll_offset_into_center(app, center_before);
        }

        self.clamp_top_pixel(app);

        // Record the new center for any future editor opening this buffer.
        let center_now = self.center_offset(app);
        self.document_id.get_mut(app).last_center_offset = center_now;

        let translate_y = -self.top_pixel as f32;

        // Maybe scroll the main cursor into view.
        if self.scroll_to_main_cursor {
            self.scroll_to_main_cursor = false;
            if let Some(cursor) = self.cursors.last() {
                self.scroll_offset_into_view(app, cursor.head.offset);
            }
        }

        // Compute the visible line range so we don't iterate the whole doc.
        let line_first = (app.atlas.grid_from_screen([0.0, self.top_pixel as f32])[1].max(0)
            as usize)
            .min(self.wraps.len());
        let line_after = (app
            .atlas
            .grid_from_screen([0.0, self.top_pixel as f32 + viewport_size[1]])[1]
            .max(0) as usize
            + 1)
        .min(self.wraps.len());

        let gutter_w = app.atlas.screen_from_grid([1, 0])[0];

        // Left gutter: soft-wrap continuation markers.
        {
            let mut drawing = drawing.push_clip_rect(Rect {
                pos: [0.0, 0.0],
                size: [gutter_w, viewport_size[1]],
            });
            let text = &self.document_id.get(app).text;
            for line_idx in line_first..line_after {
                let [start, _end] = self.wraps[line_idx];
                if start > 0 && text[start - 1] != b'\n' {
                    let mut pos = app.atlas.screen_from_grid([0, line_idx]);
                    pos[1] += translate_y;
                    drawing.draw_text(&app.atlas, BStr::new(b"\\"), pos, HIGHLIGHT_COLOR);
                }
            }
        }

        // Right gutter: viewport-indicator rect.
        {
            let mut drawing = drawing.push_clip_rect(Rect {
                pos: [viewport_size[0] - gutter_w, 0.0],
                size: [gutter_w, viewport_size[1]],
            });
            let viewport_h_f = viewport_size[1];
            let total_h = (app.atlas.screen_from_grid([0, self.wraps.len()])[1]).max(viewport_h_f);
            let top_y = ((self.top_pixel as f32) / total_h * viewport_h_f).clamp(0.0, viewport_h_f);
            let bot_y = (((self.top_pixel as f32) + viewport_h_f) / total_h * viewport_h_f)
                .clamp(0.0, viewport_h_f);
            let h = (bot_y - top_y).max(1.0);
            let gutter_size = drawing.size();
            drawing.draw_rect(
                &app.atlas,
                Rect {
                    pos: [0.0, 0.0],
                    size: gutter_size,
                },
                HIGHLIGHT_COLOR,
            );
            drawing.draw_rect(
                &app.atlas,
                Rect {
                    pos: [0.0, top_y],
                    size: [gutter_w, h],
                },
                BACKGROUND_COLOR,
            );
        }

        // Text region: marks, text, cursors.
        {
            let width = app.atlas.screen_from_grid([wrap_chars, 0])[0];
            let mut drawing = drawing.push_clip_rect(Rect {
                pos: app.atlas.screen_from_grid([1, 0]),
                size: [width, viewport_size[1]],
            });

            // Draw mark.
            if self.marked {
                for cursor in &self.cursors {
                    if let Some(range) = cursor.marked_range(self.marked) {
                        for line_idx in line_first..line_after {
                            let [wrap_start, wrap_end] = self.wraps[line_idx];
                            if range.end <= wrap_start || range.start > wrap_end {
                                continue;
                            }
                            let mark_start = range.start.max(wrap_start);
                            let mark_end = range.end.min(wrap_end);
                            let grid_start = self.grid_from_offset(app, mark_start)[1];
                            let mut grid_end = self.grid_from_offset(app, mark_end)[0];
                            grid_end[1] += 1;
                            let mut screen_start = app.atlas.screen_from_grid(grid_start);
                            let mut screen_end = app.atlas.screen_from_grid(grid_end);
                            screen_start[1] += translate_y;
                            screen_end[1] += translate_y;
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
                for line_idx in line_first..line_after {
                    let [start, end] = self.wraps[line_idx];
                    let mut screen = app.atlas.screen_from_grid([0, line_idx]);
                    screen[1] += translate_y;
                    drawing.draw_text(&app.atlas, &text.as_bstr()[start..end], screen, TEXT_COLOR);
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
                        screen_start[1] += translate_y;
                        screen_end[1] += translate_y;
                        let w = app.atlas.screen_from_grid([1, 0])[0] / 8.0;
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
    }

    fn offset_line(&self, app: &App, offset: usize) -> usize {
        self.grid_from_offset(app, offset)[1][1]
    }

    fn scroll_offset_into_view(&mut self, app: &App, offset: usize) {
        let viewport_h = self.last_viewport_size[1] as isize;
        if viewport_h <= 0 {
            return;
        }
        let line = self.offset_line(app, offset);
        let y = app.atlas.screen_from_grid([0, line])[1] as isize;
        let y_end = app.atlas.screen_from_grid([0, line + 1])[1] as isize;
        if y < self.top_pixel {
            self.top_pixel = y;
        }
        if y_end > self.top_pixel + viewport_h {
            self.top_pixel = y_end - viewport_h;
        }
    }

    fn scroll_offset_into_center(&mut self, app: &App, offset: usize) {
        let viewport_h = self.last_viewport_size[1] as isize;
        if viewport_h <= 0 {
            return;
        }
        let line = self.offset_line(app, offset);
        let y = app.atlas.screen_from_grid([0, line])[1] as isize;
        let y_end = app.atlas.screen_from_grid([0, line + 1])[1] as isize;
        self.top_pixel = (y + y_end) / 2 - viewport_h / 2;
    }

    fn center_offset(&self, app: &App) -> usize {
        let viewport_h = self.last_viewport_size[1] as isize;
        let center_y = self.top_pixel + viewport_h / 2;
        let line = app.atlas.grid_from_screen([0.0, center_y as f32])[1].max(0) as usize;
        let line = line.min(self.wraps.len() - 1);
        self.wraps[line][0]
    }

    fn clamp_top_pixel(&mut self, app: &App) {
        let viewport_h = self.last_viewport_size[1] as isize;
        let total_h = app.atlas.screen_from_grid([0, self.wraps.len()])[1] as isize;
        if viewport_h > 0 {
            let max_top = (total_h - viewport_h / 2).max(0);
            if self.top_pixel > max_top {
                self.top_pixel = max_top;
            }
        }
        if self.top_pixel < 0 {
            self.top_pixel = 0;
        }
    }

    fn offset_from_screen(&self, app: &App, screen_pos: [f32; 2]) -> usize {
        let cell_w = app.atlas.cell_size[0] as f32;
        let doc_y = screen_pos[1] + self.top_pixel as f32;
        let grid = app.atlas.grid_from_screen([screen_pos[0], doc_y]);
        // Clamp to document bounds.
        if grid[1] < 0 {
            return 0;
        }
        let line = grid[1] as usize;
        if line >= self.wraps.len() {
            return self.document_id.get(app).text.len();
        }
        // grid_from_screen returns columns where 0 = gutter, 1 = first text char.
        // Convert to wrap-relative column by subtracting the gutter (1 grid column).
        let screen_col = grid[0].max(0) as usize;
        let col = screen_col.saturating_sub(1);

        // Pixel position within this wrap-character cell.
        // The cell's left edge in screen space is at (screen_col) * cell_w.
        // But we want the offset within the wrap character's cell, whose left
        // edge is at (col + 1) * cell_w = screen_col * cell_w.
        let sub_x = screen_pos[0] - (screen_col as f32) * cell_w;
        let col = if sub_x >= cell_w / 2.0 { col + 1 } else { col };

        let [wrap_start, wrap_end] = self.wraps[line];
        let text = &self.document_id.get(app).text;
        let text_span = &text[wrap_start..wrap_end];
        // Find the byte offset at the given character column.
        let mut char_idx = 0;
        for (char_start, _char_end, _) in text_span.char_indices() {
            if char_idx >= col {
                return wrap_start + char_start;
            }
            char_idx += 1;
        }
        wrap_end
    }

    pub fn handle_edits(&mut self, app: &App, diff: &OffsetDiff) {
        let center_before = self.center_offset(app);

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

        self.scroll_offset_into_center(app, diff.apply(center_before));
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
        self.scroll_to_main_cursor = true;
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
        self.scroll_to_main_cursor = true;
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
        self.scroll_to_main_cursor = true;
    }

    fn refresh_wraps(&mut self, app: &App) {
        let text = &self.document_id.get(app).text;
        self.wraps.clear();
        compute_wraps(text.as_bstr(), self.wrap_chars, &mut self.wraps);
    }

    fn cursor_goto_line_start(&mut self, app: &App) {
        let document = self.document_id.get(app);
        for cursor in &mut self.cursors {
            cursor.head = CursorPoint {
                offset: document.line_range_from_offset(cursor.head.offset).start,
                col_wanted: None,
            };
        }
        self.scroll_to_main_cursor = true;
    }

    fn cursor_goto_line_end(&mut self, app: &App) {
        let document = self.document_id.get(app);
        for cursor in &mut self.cursors {
            cursor.head = CursorPoint {
                offset: document.line_range_from_offset(cursor.head.offset).end,
                col_wanted: None,
            };
        }
        self.scroll_to_main_cursor = true;
    }

    fn cursor_goto_doc_start(&mut self, app: &App) {
        for cursor in &mut self.cursors {
            cursor.head = CursorPoint {
                offset: 0,
                col_wanted: None,
            };
        }
        self.scroll_offset_into_view(app, 0);
    }

    fn cursor_goto_doc_end(&mut self, app: &App) {
        let end = self.document_id.get(app).text.len();
        for cursor in &mut self.cursors {
            cursor.head = CursorPoint {
                offset: end,
                col_wanted: None,
            };
        }
        self.scroll_offset_into_center(app, end);
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
        self.scroll_to_main_cursor = true;
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

    fn editor_with_text(text: &str, wrap_chars: usize) -> (crate::app::App, crate::app::EditorId) {
        use crate::fuzz::MockIO;
        let mut io = MockIO::new();
        let initial = io.fresh_window_id();
        io.open_windows.push(initial);
        let app = crate::app::App::new(initial, &mut io);
        let document_id = *app.documents.keys().next().unwrap();
        let editor_id = *app.editors.keys().next().unwrap();
        if !text.is_empty() {
            document_id.get_mut(&app).apply_edits(&[Edit {
                kind: EditKind::Insert,
                offset: 0,
                text: text.into(),
            }]);
        }
        {
            let mut editor = editor_id.get_mut(&app);
            editor.wrap_chars = wrap_chars;
            editor.refresh_wraps(&app);
            // The scroll helpers bail when viewport_h == 0, so give the editor
            // a viewport big enough for the small docs used in these tests.
            editor.last_viewport_size = [800.0, 600.0];
        }
        (app, editor_id)
    }

    fn set_heads(editor: &mut Editor, heads: &[usize]) {
        editor.cursors = heads
            .iter()
            .map(|&offset| Cursor {
                head: CursorPoint {
                    offset,
                    col_wanted: None,
                },
                tail: CursorPoint {
                    offset,
                    col_wanted: None,
                },
            })
            .collect();
    }

    fn head_offsets(editor: &Editor) -> Vec<usize> {
        editor.cursors.iter().map(|c| c.head.offset).collect()
    }

    #[test]
    fn alt_l_moves_to_end_of_real_line() {
        let (app, editor_id) = editor_with_text("abc\ndefg\nhij", 80);
        let mut editor = editor_id.get_mut(&app);
        set_heads(&mut editor, &[1]);
        editor.cursor_goto_line_end(&app);
        assert_eq!(head_offsets(&editor), vec![3]);
    }

    #[test]
    fn alt_l_at_end_of_line_stays() {
        let (app, editor_id) = editor_with_text("abc\ndef", 80);
        let mut editor = editor_id.get_mut(&app);
        set_heads(&mut editor, &[3]);
        editor.cursor_goto_line_end(&app);
        assert_eq!(head_offsets(&editor), vec![3]);
    }

    #[test]
    fn alt_l_crosses_soft_wrap_to_end_of_real_line() {
        // Real line "abcdefg" soft-wraps into ["abc","def","g"] at wrap_chars=3.
        // From a cursor on the first soft-wrapped row, Alt+l must reach the
        // *real* line end (offset 7), not the soft-wrap end (offset 3).
        let (app, editor_id) = editor_with_text("abcdefg", 3);
        let mut editor = editor_id.get_mut(&app);
        assert!(editor.wraps.len() > 1, "test requires a soft wrap");
        set_heads(&mut editor, &[1]);
        editor.cursor_goto_line_end(&app);
        assert_eq!(head_offsets(&editor), vec![7]);
    }

    #[test]
    fn alt_j_moves_to_start_of_real_line() {
        let (app, editor_id) = editor_with_text("abc\ndefg\nhij", 80);
        let mut editor = editor_id.get_mut(&app);
        set_heads(&mut editor, &[6]);
        editor.cursor_goto_line_start(&app);
        assert_eq!(head_offsets(&editor), vec![4]);
    }

    #[test]
    fn alt_j_at_start_of_line_stays() {
        let (app, editor_id) = editor_with_text("abc\ndef", 80);
        let mut editor = editor_id.get_mut(&app);
        set_heads(&mut editor, &[4]);
        editor.cursor_goto_line_start(&app);
        assert_eq!(head_offsets(&editor), vec![4]);
    }

    #[test]
    fn alt_j_crosses_soft_wrap_to_start_of_real_line() {
        // Same soft-wrap setup as above: from a cursor in the middle of
        // a soft-wrapped row, Alt+j must reach the real line start (0),
        // not the soft-wrap start.
        let (app, editor_id) = editor_with_text("abcdefg", 3);
        let mut editor = editor_id.get_mut(&app);
        assert!(editor.wraps.len() > 1, "test requires a soft wrap");
        set_heads(&mut editor, &[5]);
        editor.cursor_goto_line_start(&app);
        assert_eq!(head_offsets(&editor), vec![0]);
    }

    #[test]
    fn alt_l_moves_every_cursor() {
        let (app, editor_id) = editor_with_text("abc\ndefg\nhij", 80);
        let mut editor = editor_id.get_mut(&app);
        set_heads(&mut editor, &[1, 5, 10]);
        editor.cursor_goto_line_end(&app);
        assert_eq!(head_offsets(&editor), vec![3, 8, 12]);
    }

    #[test]
    fn alt_j_moves_every_cursor() {
        let (app, editor_id) = editor_with_text("abc\ndefg\nhij", 80);
        let mut editor = editor_id.get_mut(&app);
        set_heads(&mut editor, &[2, 6, 11]);
        editor.cursor_goto_line_start(&app);
        assert_eq!(head_offsets(&editor), vec![0, 4, 9]);
    }

    #[test]
    fn alt_i_moves_all_cursors_to_doc_start_and_scrolls_to_top() {
        let (app, editor_id) = editor_with_text("abc\ndef", 80);
        let mut editor = editor_id.get_mut(&app);
        set_heads(&mut editor, &[2, 5]);
        editor.top_pixel = 100;
        editor.cursor_goto_doc_start(&app);
        assert_eq!(head_offsets(&editor), vec![0, 0]);
        assert_eq!(editor.top_pixel, 0);
    }

    #[test]
    fn alt_k_moves_all_cursors_to_doc_end() {
        let (app, editor_id) = editor_with_text("abc\ndef", 80);
        let mut editor = editor_id.get_mut(&app);
        set_heads(&mut editor, &[0, 2]);
        editor.cursor_goto_doc_end(&app);
        assert_eq!(head_offsets(&editor), vec![7, 7]);
    }

    // Build a modifiers state with (or without) Ctrl.
    #[test]
    fn offset_from_screen_left_half_of_first_char_returns_zero() {
        let (app, editor_id) = editor_with_text("hello\nworld", 80);
        let editor = editor_id.get_mut(&app);
        let cell_w = app.atlas.cell_size[0] as f32;
        // Screen columns: 0=gutter, 1=h
        // Left half of screen col 1 (first char 'h') → before first char = offset 0.
        assert_eq!(editor.offset_from_screen(&app, [0.0, 0.0]), 0);
        assert!(
            editor.offset_from_screen(&app, [1.0 * cell_w + cell_w / 4.0, 0.0]) == 0,
            "left half of first char should be offset 0"
        );
    }

    #[test]
    fn offset_from_screen_right_half_of_first_char_returns_end_of_first_char() {
        let (app, editor_id) = editor_with_text("hello\nworld", 80);
        let editor = editor_id.get_mut(&app);
        let cell_w = app.atlas.cell_size[0] as f32;
        // Screen columns: 0=gutter, 1=h, 2=e
        // Right half of screen col 1 ('h') → after first char = offset 1.
        let x = 1.0 * cell_w + cell_w / 2.0 + 1.0;
        assert_eq!(editor.offset_from_screen(&app, [x, 0.0]), 1);
    }

    #[test]
    fn offset_from_screen_right_half_of_second_char_returns_two() {
        let (app, editor_id) = editor_with_text("hello", 80);
        let editor = editor_id.get_mut(&app);
        let cell_w = app.atlas.cell_size[0] as f32;
        // Screen columns: 0=gutter, 1=h, 2=e
        // Right half of screen col 2 ('e') → after 'e' = offset 2.
        let x = 2.0 * cell_w + cell_w / 2.0 + 1.0;
        assert_eq!(editor.offset_from_screen(&app, [x, 0.0]), 2);
    }

    #[test]
    fn offset_from_screen_maps_second_line_correctly() {
        let (app, editor_id) = editor_with_text("hello\nworld", 80);
        let editor = editor_id.get_mut(&app);
        let cell_h = app.atlas.cell_size[1] as f32;
        assert_eq!(editor.offset_from_screen(&app, [0.0, cell_h]), 6);
    }

    #[test]
    fn offset_from_screen_above_doc_returns_zero() {
        let (app, editor_id) = editor_with_text("hello", 80);
        let editor = editor_id.get_mut(&app);
        assert_eq!(editor.offset_from_screen(&app, [0.0, -100.0]), 0);
    }

    #[test]
    fn offset_from_screen_below_doc_returns_end() {
        let (app, editor_id) = editor_with_text("hello", 80);
        let editor = editor_id.get_mut(&app);
        assert_eq!(editor.offset_from_screen(&app, [0.0, 10000.0]), 5);
    }

    #[test]
    fn click_clears_other_cursors_and_sets_single_cursor() {
        let (mut app, editor_id) = editor_with_text("hello world", 80);
        app.modifiers = winit::keyboard::ModifiersState::empty();
        let mut editor = editor_id.get_mut(&app);
        set_heads(&mut editor, &[0, 6]);
        editor.dragging = None;
        let mut io = crate::fuzz::MockIO::new();
        editor.input(
            &app,
            &mut io,
            InputEvent::MouseButton {
                state: ElementState::Pressed,
                position: [0.0, 0.0],
            },
        );
        assert_eq!(editor.cursors.len(), 1);
        assert_eq!(editor.cursors[0].head.offset, 0);
        assert_eq!(editor.cursors[0].tail.offset, 0);
        assert!(!editor.marked);
        assert!(editor.dragging.is_some());
    }

    #[test]
    fn ctrl_click_adds_new_cursor() {
        let (mut app, editor_id) = editor_with_text("hello world", 80);
        {
            let mut m = winit::keyboard::ModifiersState::empty();
            m |= winit::keyboard::ModifiersState::CONTROL;
            app.modifiers = m;
        }
        let mut editor = editor_id.get_mut(&app);
        set_heads(&mut editor, &[0]);
        // Screen columns: 0=gutter, 1=h, 2=e, 3=l, 4=l, 5=o, 6=' ', 7=w
        let cell_w = app.atlas.cell_size[0] as f32;
        let world_x = 7.0 * cell_w;
        let mut io = crate::fuzz::MockIO::new();
        editor.input(
            &app,
            &mut io,
            InputEvent::MouseButton {
                state: ElementState::Pressed,
                position: [world_x, 0.0],
            },
        );
        assert_eq!(editor.cursors.len(), 2);
        assert_eq!(editor.cursors[1].head.offset, 6);
    }

    #[test]
    fn drag_creates_selection() {
        let (mut app, editor_id) = editor_with_text("hello world", 80);
        app.modifiers = winit::keyboard::ModifiersState::empty();
        let mut editor = editor_id.get_mut(&app);
        let cell_w = app.atlas.cell_size[0] as f32;
        // Screen columns: 0=gutter, 1=h, 2=e, 3=l, 4=l, 5=o, 6=' ', 7=w
        let world_x = 7.0 * cell_w;
        let mut io = crate::fuzz::MockIO::new();
        editor.input(
            &app,
            &mut io,
            InputEvent::MouseButton {
                state: ElementState::Pressed,
                position: [world_x, 0.0],
            },
        );
        assert_eq!(editor.cursors.len(), 1);
        assert_eq!(editor.cursors[0].head.offset, 6);
        // 'o' in "hello" is char idx 4 at screen col 5
        let hello_mid_x = 5.0 * cell_w;
        let mut io2 = crate::fuzz::MockIO::new();
        io2.mouse_pos = [hello_mid_x, 0.0];
        let mut redraw = false;
        editor.tick(&app, &mut io2, &mut redraw);
        assert!(redraw);
        assert!(editor.dragging.is_some());
        assert_eq!(editor.cursors[0].head.offset, 4);
        assert_eq!(editor.cursors[0].tail.offset, 6);
    }

    #[test]
    fn ctrl_drag_adds_cursor_with_selection() {
        let (mut app, editor_id) = editor_with_text("hello world", 80);
        {
            let mut m = winit::keyboard::ModifiersState::empty();
            m |= winit::keyboard::ModifiersState::CONTROL;
            app.modifiers = m;
        }
        let mut editor = editor_id.get_mut(&app);
        set_heads(&mut editor, &[0]);
        let cell_w = app.atlas.cell_size[0] as f32;
        // Screen columns: 0=gutter, 1=h, 2=e, 3=l, 4=l, 5=o, 6=' ', 7=w
        let world_x = 7.0 * cell_w;
        let mut io = crate::fuzz::MockIO::new();
        editor.input(
            &app,
            &mut io,
            InputEvent::MouseButton {
                state: ElementState::Pressed,
                position: [world_x, 0.0],
            },
        );
        assert_eq!(editor.cursors.len(), 2);
        assert_eq!(editor.cursors[1].head.offset, 6);
        // 'o' in "hello" is char idx 4 at screen col 5
        let hello_mid_x = 5.0 * cell_w;
        let mut io2 = crate::fuzz::MockIO::new();
        io2.mouse_pos = [hello_mid_x, 0.0];
        let mut redraw = false;
        editor.tick(&app, &mut io2, &mut redraw);
        assert!(redraw);
        assert_eq!(editor.cursors.len(), 2);
        assert_eq!(editor.cursors[1].head.offset, 4);
        assert_eq!(editor.cursors[1].tail.offset, 6);
    }

    #[test]
    fn drag_release_clears_drag_state() {
        let (mut app, editor_id) = editor_with_text("hello", 80);
        app.modifiers = winit::keyboard::ModifiersState::empty();
        let mut editor = editor_id.get_mut(&app);
        let mut io = crate::fuzz::MockIO::new();
        editor.input(
            &app,
            &mut io,
            InputEvent::MouseButton {
                state: ElementState::Pressed,
                position: [0.0, 0.0],
            },
        );
        assert!(editor.dragging.is_some());
        editor.input(
            &app,
            &mut io,
            InputEvent::MouseButton {
                state: ElementState::Released,
                position: [0.0, 0.0],
            },
        );
        assert!(editor.dragging.is_none());
    }

    #[test]
    fn drag_off_screen_bottom_scrolls_down() {
        // Long enough doc that scrolling down is possible.
        let text: String = (0..50).map(|i| format!("line{}\n", i)).collect();
        let (mut app, editor_id) = editor_with_text(&text, 80);
        app.modifiers = winit::keyboard::ModifiersState::empty();
        let mut editor = editor_id.get_mut(&app);
        editor.top_pixel = 0;
        let mut io = crate::fuzz::MockIO::new();
        editor.input(
            &app,
            &mut io,
            InputEvent::MouseButton {
                state: ElementState::Pressed,
                position: [0.0, 0.0],
            },
        );
        let top_before = editor.top_pixel;
        io.mouse_pos = [0.0, editor.last_viewport_size[1] + 50.0];
        let mut redraw = false;
        editor.tick(&app, &mut io, &mut redraw);
        assert!(editor.top_pixel > top_before, "expected scroll down");
    }

    #[test]
    fn drag_off_screen_top_scrolls_up() {
        // Long enough doc that we can scroll without hitting the bottom edge.
        let text: String = (0..50).map(|i| format!("line{}\n", i)).collect();
        let (mut app, editor_id) = editor_with_text(&text, 80);
        app.modifiers = winit::keyboard::ModifiersState::empty();
        let mut editor = editor_id.get_mut(&app);
        editor.top_pixel = 100;
        let mut io = crate::fuzz::MockIO::new();
        editor.input(
            &app,
            &mut io,
            InputEvent::MouseButton {
                state: ElementState::Pressed,
                position: [0.0, 50.0],
            },
        );
        let top_before = editor.top_pixel;
        io.mouse_pos = [0.0, -50.0];
        let mut redraw = false;
        editor.tick(&app, &mut io, &mut redraw);
        assert!(editor.top_pixel < top_before, "expected scroll up");
    }

    #[test]
    fn alt_k_scrolls_viewport_to_end() {
        // Long enough doc that the end is below the initial viewport.
        let text: String = (0..50).map(|i| format!("line{}\n", i)).collect();
        let (app, editor_id) = editor_with_text(&text, 80);
        let mut editor = editor_id.get_mut(&app);
        editor.top_pixel = 0;
        editor.cursor_goto_doc_end(&app);
        assert!(editor.top_pixel > 0, "expected to scroll down");
        // The last line is centered in the viewport.
        let last_line = editor.wraps.len() - 1;
        let y = app.atlas.screen_from_grid([0, last_line])[1] as isize;
        let y_end = app.atlas.screen_from_grid([0, last_line + 1])[1] as isize;
        let viewport_h = editor.last_viewport_size[1] as isize;
        assert_eq!(editor.top_pixel, (y + y_end) / 2 - viewport_h / 2);
    }
}
