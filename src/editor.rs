use std::mem::swap;
use std::time::Duration;
use std::{mem::replace, ops::Range};

use bstr::{BStr, ByteSlice};
use winit::{
    event::ElementState,
    keyboard::{Key, NamedKey},
};

use crate::style::BACKGROUND_COLOR;
use crate::{
    app::{App, DocumentId, EditorId, IO, InputEvent},
    document::{Document, Edit, EditKind, OffsetDiff, SaveKind},
    drawing::{Drawing, Rect},
    style::{HIGHLIGHT_COLOR, MULTI_CURSOR_COLOR, TEXT_COLOR},
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

#[derive(Clone)]
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
    pub(crate) fn new(document_id: DocumentId, app: &App) -> Self {
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

    pub fn assert_invariants(&self, app: &App) {
        let document = self.document_id.get(app);

        assert!(self.cursors.len() > 0);
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

    pub fn input(editor_id: EditorId, app: &App, io: &mut dyn IO, event: InputEvent) {
        let mut editor = editor_id.get_mut(app);
        match event {
            InputEvent::Key {
                state, logical_key, ..
            } if state == ElementState::Pressed
                && app.modifiers.control_key()
                && !app.modifiers.alt_key() =>
            {
                match logical_key.as_ref() {
                    Key::Character("i") => {
                        editor.cursor_move(app, Direction::Up);
                    }
                    Key::Character("k") => {
                        editor.cursor_move(app, Direction::Down);
                    }
                    Key::Character("j") => {
                        editor.cursor_move(app, Direction::Left);
                    }
                    Key::Character("l") => {
                        editor.cursor_move(app, Direction::Right);
                    }
                    Key::Named(NamedKey::Space) => {
                        editor.toggle_mark();
                    }
                    Key::Character("d") => {
                        editor.cursor_add_next_match(app);
                    }
                    Key::Character("D") => {
                        editor.cursor_remove_last();
                    }
                    Key::Character("c") => {
                        editor.cursor_copy(app, io);
                    }
                    Key::Character("x") => {
                        editor.cursor_cut(app, io);
                    }
                    Key::Character("v") => {
                        editor.cursor_paste(app, io);
                    }
                    Key::Character("V") => {
                        editor.cursor_paste_many(app, io);
                    }
                    Key::Character("s") => {
                        editor.save(app, io, SaveKind::Explicit);
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
                        editor.cursor_goto_line_start(app);
                    }
                    Key::Character("l") => {
                        editor.cursor_goto_line_end(app);
                    }
                    Key::Character("i") => {
                        editor.cursor_goto_doc_start(app);
                    }
                    Key::Character("k") => {
                        editor.cursor_goto_doc_end(app);
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
                    Key::Character(char) => editor.cursor_replace(app, io, char.as_bytes()),
                    Key::Named(NamedKey::Enter) => editor.cursor_replace(app, io, b"\n"),
                    Key::Named(NamedKey::Space) => editor.cursor_replace(app, io, b" "),
                    Key::Named(NamedKey::Backspace) => editor.cursor_delete_left(app, io),
                    Key::Named(NamedKey::Delete) => editor.cursor_delete_right(app, io),
                    _ => {}
                }
            }
            InputEvent::MouseButton { state, position } => {
                match state {
                    ElementState::Pressed => {
                        let offset = editor.offset_from_screen(app, position);

                        // Ctrl-click / Ctrl-drag: add a new cursor
                        // Click / drag: set main cursor, remove others
                        if !app.modifiers.control_key() {
                            editor.cursors.clear();
                        }

                        let idx = editor.cursors.len();
                        editor.cursors.push(Cursor {
                            head: CursorPoint {
                                offset,
                                col_wanted: None,
                            },
                            tail: CursorPoint {
                                offset,
                                col_wanted: None,
                            },
                        });
                        editor.dragging = Some(DragInfo { cursor_index: idx });
                        editor.marked = false;
                        editor.scroll_to_main_cursor = true;
                    }
                    ElementState::Released => {
                        editor.dragging = None;
                    }
                }
            }
            InputEvent::MouseWheel { y_offset } => {
                editor.top_pixel -= (SCROLL_AMOUNT * y_offset) as isize;
            }
            InputEvent::FocusChanged { focused: false } => {
                editor.save(app, io, SaveKind::Auto);
            }
            _ => {}
        }
        editor.last_input = io.frame_start();
    }

    fn save(&self, app: &App, io: &mut dyn IO, kind: SaveKind) {
        Document::save(self.document_id, app, io, kind);
    }

    pub fn tick(editor_id: EditorId, app: &App, io: &mut dyn IO) {
        let mut editor = editor_id.get_mut(app);
        Document::tick(editor.document_id, app, io);

        // During drag, poll mouse position and update cursor head.
        if let Some(drag_info) = editor.dragging {
            let mouse_pos = io.mouse_position();
            // Scroll when mouse is off-screen vertically.
            if mouse_pos[1] < 0.0 {
                editor.top_pixel -= SCROLL_AMOUNT as isize;
            } else if mouse_pos[1] > editor.last_viewport_size[1] {
                editor.top_pixel += SCROLL_AMOUNT as isize;
            }
            editor.clamp_top_pixel(app);

            // Convert screen position to document offset, accounting for scroll.
            let offset = editor.offset_from_screen(app, mouse_pos);
            if let Some(cursor) = editor.cursors.get_mut(drag_info.cursor_index) {
                let moved = cursor.head.offset != offset;
                cursor.head = CursorPoint {
                    offset,
                    col_wanted: None,
                };
                if moved {
                    editor.marked = true;
                }
            }

            editor.last_input = io.frame_start();
        }

        editor.show_cursor = ((io.frame_start().as_millis() / 500) % 2) == 0
            || (io.frame_start() - editor.last_input < Duration::from_millis(500));
    }

    pub fn draw(editor_id: EditorId, app: &App, drawing: &mut Drawing) {
        let mut editor = editor_id.get_mut(app);
        // Update wrapping.
        let viewport_size = drawing.size();
        let grid_w = app.atlas.grid_from_screen(viewport_size)[0] as usize;
        if grid_w <= 2 {
            return;
        }
        // Leave space for gutters
        let wrap_chars = grid_w - 2;

        let center_before = editor.center_offset(app);

        if editor.wrap_chars != wrap_chars {
            editor.wrap_chars = wrap_chars;
            editor.refresh_wraps(app);
        }

        let viewport_changed = editor.last_viewport_size != viewport_size;
        editor.last_viewport_size = viewport_size;

        // Handle viewport resize: re-center on the same logical position.
        if viewport_changed {
            let center_before = center_before.min(editor.document_id.get(app).text.len());
            editor.scroll_offset_into_center(app, center_before);
        }

        editor.clamp_top_pixel(app);

        // Record the new center for any future editor opening this buffer.
        let center_now = editor.center_offset(app);
        editor.document_id.get_mut(app).last_center_offset = center_now;

        let translate_y = -editor.top_pixel as f32;

        // Maybe scroll the main cursor into view.
        if editor.scroll_to_main_cursor {
            editor.scroll_to_main_cursor = false;
            if let Some(offset) = editor.cursors.last().map(|cursor| cursor.head.offset) {
                editor.scroll_offset_into_view(app, offset);
            }
        }

        // Compute the visible line range so we don't iterate the whole doc.
        let line_first = (app.atlas.grid_from_screen([0.0, editor.top_pixel as f32])[1].max(0)
            as usize)
            .min(editor.wraps.len());
        let line_after = (app
            .atlas
            .grid_from_screen([0.0, editor.top_pixel as f32 + viewport_size[1]])[1]
            .max(0) as usize
            + 1)
        .min(editor.wraps.len());

        let gutter_w = app.atlas.screen_from_grid([1, 0])[0];

        // Left gutter: soft-wrap continuation markers.
        {
            let mut drawing = drawing.push_clip_rect(Rect {
                pos: [0.0, 0.0],
                size: [gutter_w, viewport_size[1]],
            });
            let text = &editor.document_id.get(app).text;
            for line_idx in line_first..line_after {
                let [start, _end] = editor.wraps[line_idx];
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
            let total_h =
                (app.atlas.screen_from_grid([0, editor.wraps.len()])[1]).max(viewport_h_f);
            let top_y =
                ((editor.top_pixel as f32) / total_h * viewport_h_f).clamp(0.0, viewport_h_f);
            let bot_y = (((editor.top_pixel as f32) + viewport_h_f) / total_h * viewport_h_f)
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
            if editor.marked {
                for cursor in &editor.cursors {
                    if let Some(range) = cursor.marked_range(editor.marked) {
                        for line_idx in line_first..line_after {
                            let [wrap_start, wrap_end] = editor.wraps[line_idx];
                            if range.end <= wrap_start || range.start > wrap_end {
                                continue;
                            }
                            let mark_start = range.start.max(wrap_start);
                            let mark_end = range.end.min(wrap_end);
                            let grid_start = editor.grid_from_offset(app, mark_start)[1];
                            let mut grid_end = editor.grid_from_offset(app, mark_end)[0];
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
                let text = &editor.document_id.get(app).text;
                for line_idx in line_first..line_after {
                    let [start, end] = editor.wraps[line_idx];
                    let mut screen = app.atlas.screen_from_grid([0, line_idx]);
                    screen[1] += translate_y;
                    drawing.draw_text(&app.atlas, &text.as_bstr()[start..end], screen, TEXT_COLOR);
                }
            }

            // Draw cursors.
            if editor.show_cursor {
                let cursor_color = if editor.cursors.len() > 1 {
                    MULTI_CURSOR_COLOR
                } else {
                    TEXT_COLOR
                };
                for cursor in &editor.cursors {
                    for grid_start in editor.grid_from_offset(app, cursor.head.offset) {
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
                            cursor_color,
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
        if screen_col == 0 {
            return self.wraps[line][0];
        }

        let col = screen_col - 1;

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

    pub fn handle_edits(editor_id: EditorId, app: &App, diff: &OffsetDiff) {
        let mut editor = editor_id.get_mut(app);
        let center_before = editor.center_offset(app);

        let mut cursors = replace(&mut editor.cursors, vec![]);
        for cursor in &mut cursors {
            for point in [&mut cursor.head, &mut cursor.tail] {
                *point = CursorPoint {
                    offset: diff.apply(point.offset),
                    col_wanted: None,
                };
            }
        }
        editor.cursors = cursors;
        editor.refresh_wraps(app);

        editor.scroll_offset_into_center(app, diff.apply(center_before));
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

    fn cursor_replace(&mut self, app: &App, io: &mut dyn IO, insert: &[u8]) {
        let document = self.document_id.get(app);
        let mut edits = Vec::with_capacity(self.cursors.len() * 2);
        for cursor in &mut self.cursors {
            if let Some(range) = cursor.marked_range(self.marked) {
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
                cursor.head.offset = range.start;
                cursor.tail.offset = range.start;
            } else {
                edits.push(Edit {
                    kind: EditKind::Insert,
                    offset: cursor.head.offset,
                    text: insert.into(),
                });
            }
        }
        Edit::coalesce(&mut edits);
        drop(document);
        Document::queue_edits(self.document_id, app, io, edits);
        self.marked = false;
        self.scroll_to_main_cursor = true;
    }

    fn cursor_add_next_match(&mut self, app: &App) {
        let document = self.document_id.get(app);
        let cursor_main = self.cursors.last().unwrap();
        let Some(range) = cursor_main.marked_range(self.marked) else {
            return;
        };
        let search_start = range.end;
        let search_text = &document.text[range];
        if let Some(offset) = document.text[search_start..].find(search_text.as_bstr()) {
            let start = search_start + offset;
            let end = start + search_text.len();
            let mut cursor_new = Cursor {
                head: CursorPoint {
                    offset: end,
                    col_wanted: None,
                },
                tail: CursorPoint {
                    offset: start,
                    col_wanted: None,
                },
            };
            if cursor_main.head.offset < cursor_main.tail.offset {
                swap(&mut cursor_new.head, &mut cursor_new.tail);
            }
            self.cursors.push(cursor_new);
        }
    }

    fn cursor_remove_last(&mut self) {
        if self.cursors.len() > 1 {
            self.cursors.pop();
        }
    }

    fn cursor_delete_left(&mut self, app: &App, io: &mut dyn IO) {
        let document = self.document_id.get(app);
        let mut edits = Vec::with_capacity(self.cursors.len());
        for cursor in &self.cursors {
            if let Some(range) = cursor.marked_range(self.marked) {
                edits.push(Edit {
                    kind: EditKind::Delete,
                    offset: range.start,
                    text: document.text[range.start..range.end].into(),
                });
            } else if let Some(start) =
                Document::char_prev(self.document_id, app, cursor.head.offset)
            {
                edits.push(Edit {
                    kind: EditKind::Delete,
                    offset: start,
                    text: document.text[start..cursor.head.offset].into(),
                });
            }
        }
        Edit::coalesce(&mut edits);
        drop(document);
        Document::queue_edits(self.document_id, app, io, edits);
        self.marked = false;
        self.scroll_to_main_cursor = true;
    }

    fn cursor_delete_right(&mut self, app: &App, io: &mut dyn IO) {
        let document = self.document_id.get(app);
        let mut edits = Vec::with_capacity(self.cursors.len());
        for cursor in &self.cursors {
            if let Some(range) = cursor.marked_range(self.marked) {
                edits.push(Edit {
                    kind: EditKind::Delete,
                    offset: range.start,
                    text: document.text[range.start..range.end].into(),
                });
            } else if let Some(end) = Document::char_next(self.document_id, app, cursor.head.offset)
            {
                edits.push(Edit {
                    kind: EditKind::Delete,
                    offset: cursor.head.offset,
                    text: document.text[cursor.head.offset..end].into(),
                });
            }
        }
        Edit::coalesce(&mut edits);
        drop(document);
        Document::queue_edits(self.document_id, app, io, edits);
        self.marked = false;
        self.scroll_to_main_cursor = true;
    }

    fn cursor_copy(&self, app: &App, io: &mut dyn IO) {
        let document = self.document_id.get(app);
        let texts: Vec<&[u8]> = self
            .cursors
            .iter()
            .filter_map(|cursor| cursor.marked_range(self.marked))
            .map(|range| &document.text[range])
            .collect();
        if texts.is_empty() {
            return;
        }

        // Join multiple selections with newlines.
        let mut joined = texts[0].to_vec();
        for t in &texts[1..] {
            joined.push(b'\n');
            joined.extend_from_slice(t);
        }

        // Convert to String.
        if let Ok(s) = String::from_utf8(joined) {
            io.set_clipboard_text(s);
        }
    }

    fn cursor_cut(&mut self, app: &App, io: &mut dyn IO) {
        self.cursor_copy(app, io);

        let document = self.document_id.get(app);
        let mut edits = Vec::with_capacity(self.cursors.len());
        for cursor in &self.cursors {
            if let Some(range) = cursor.marked_range(self.marked) {
                edits.push(Edit {
                    kind: EditKind::Delete,
                    offset: range.start,
                    text: document.text[range.start..range.end].into(),
                });
            }
        }
        Edit::coalesce(&mut edits);
        drop(document);
        Document::queue_edits(self.document_id, app, io, edits);
        self.marked = false;
        self.scroll_to_main_cursor = true;
    }

    fn cursor_paste(&mut self, app: &App, io: &mut dyn IO) {
        let Some(clip_text) = io.get_clipboard_text() else {
            return;
        };
        let document = self.document_id.get(app);
        let mut edits = Vec::with_capacity(self.cursors.len() * 2);
        for cursor in &self.cursors {
            if let Some(range) = cursor.marked_range(self.marked) {
                // Replace selection with clipboard.
                edits.push(Edit {
                    kind: EditKind::Insert,
                    offset: range.start,
                    text: clip_text.as_bytes().into(),
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
                    text: clip_text.as_bytes().into(),
                });
            }
        }
        Edit::coalesce(&mut edits);
        drop(document);
        Document::queue_edits(self.document_id, app, io, edits);
        self.marked = false;
        self.scroll_to_main_cursor = true;
    }

    fn cursor_paste_many(&mut self, app: &App, io: &mut dyn IO) {
        let Some(clip_text) = io.get_clipboard_text() else {
            return;
        };
        let lines: Vec<&str> = clip_text.split('\n').collect();
        let document = self.document_id.get(app);
        let mut edits = Vec::new();
        for (cursor, line) in self.cursors.iter_mut().zip(lines) {
            if let Some(range) = cursor.marked_range(self.marked) {
                // Replace selection with this line.
                edits.push(Edit {
                    kind: EditKind::Insert,
                    offset: range.start,
                    text: line.into(),
                });
                edits.push(Edit {
                    kind: EditKind::Delete,
                    offset: range.start,
                    text: document.text[range.start..range.end].into(),
                });
                cursor.head.offset = range.start;
                cursor.tail.offset = range.start;
            } else {
                edits.push(Edit {
                    kind: EditKind::Insert,
                    offset: cursor.head.offset,
                    text: line.into(),
                });
            }
        }
        Edit::coalesce(&mut edits);
        drop(document);
        Document::queue_edits(self.document_id, app, io, edits);
        self.marked = false;
        self.scroll_to_main_cursor = true;
    }

    fn refresh_wraps(&mut self, app: &App) {
        let text = &self.document_id.get(app).text;
        self.wraps.clear();
        assert!(self.wrap_chars > 0);

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
                if col >= self.wrap_chars {
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
            self.wraps.push([start, end]);
            if end >= text.len() {
                break;
            }
            if newline {
                end += 1
            }
        }
    }

    fn cursor_goto_line_start(&mut self, app: &App) {
        for cursor in &mut self.cursors {
            cursor.head = CursorPoint {
                offset: Document::line_range_from_offset(self.document_id, app, cursor.head.offset)
                    .start,
                col_wanted: None,
            };
        }
        self.scroll_to_main_cursor = true;
    }

    fn cursor_goto_line_end(&mut self, app: &App) {
        for cursor in &mut self.cursors {
            cursor.head = CursorPoint {
                offset: Document::line_range_from_offset(self.document_id, app, cursor.head.offset)
                    .end,
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
        let mut cursors = replace(&mut self.cursors, vec![]);
        for cursor in &mut cursors {
            match direction {
                Direction::Left => {
                    cursor.head = CursorPoint {
                        offset: Document::char_prev(self.document_id, app, cursor.head.offset)
                            .unwrap_or(cursor.head.offset),
                        col_wanted: None,
                    };
                }
                Direction::Right => {
                    cursor.head = CursorPoint {
                        offset: Document::char_next(self.document_id, app, cursor.head.offset)
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
