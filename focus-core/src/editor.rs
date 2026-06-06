use std::mem::{swap, take};
use std::time::Duration;
use std::{mem::replace, ops::Range};

use bstr::{BStr, ByteSlice};

use crate::input::{ElementState, InputEvent, Key, NamedKey};
use crate::style::BACKGROUND_COLOR;
use crate::{
    app::{App, IO},
    document::{DocumentId, Edit, EditKind, OffsetDiff, SaveKind},
    drawing::{Drawing, Rect},
    style::{HIGHLIGHT_COLOR, MULTI_CURSOR_COLOR, TEXT_COLOR},
};

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy, Debug)]
pub struct EditorId(pub(crate) usize);

pub struct Editor {
    pub(crate) document_id: DocumentId,
    cursors: Vec<Cursor>,
    marked: bool,
    show_cursor: bool,
    wrap_chars: usize,
    wraps: Vec<[usize; 2]>,
    last_input: Duration,
    top_pixel: isize,
    last_viewport_size: [f32; 2],
    is_dragging: bool,
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

impl Editor {
    pub(crate) fn new(app: &App, document_id: DocumentId) -> Self {
        let wrap_chars = 80;
        let wraps = wraps_from_text(document_id.text(app).as_bstr(), wrap_chars);
        Editor {
            document_id: document_id,
            cursors: vec![Cursor {
                head: CursorPoint::new(0),
                tail: CursorPoint::new(0),
            }],
            marked: false,
            show_cursor: true,
            wrap_chars,
            wraps,
            last_input: Duration::ZERO,
            top_pixel: 0,
            last_viewport_size: [0.0, 0.0],
            is_dragging: false,
        }
    }
}

impl EditorId {
    pub(crate) fn get<'a>(self, app: &'a App) -> &'a Editor {
        app.editors.get(&self).unwrap()
    }

    pub(crate) fn get_mut<'a>(self, app: &'a mut App) -> &'a mut Editor {
        app.editors.get_mut(&self).unwrap()
    }

    pub(crate) fn assert_invariants(self, app: &App) {
        let editor = self.get(app);
        let text = editor.document_id.text(app);

        // Cursors
        assert!(editor.cursors.len() > 0);
        for cursor in &editor.cursors {
            for point in [&cursor.head, &cursor.tail] {
                assert!(point.offset <= text.len());
            }
        }

        // Wraps
        assert!(!editor.wraps.is_empty());
        assert_eq!(editor.wraps[0][0], 0);
        assert_eq!(editor.wraps.last().unwrap()[1], text.len());
        for wrap in &editor.wraps {
            assert!(wrap[0] <= wrap[1]);
            assert!(text[wrap[0]..wrap[1]].chars().count() <= editor.wrap_chars)
        }
        for pair in editor.wraps.windows(2) {
            let gap = pair[1][0] - pair[0][1];
            assert!(gap <= 1);
            if gap == 1 {
                assert!(text[pair[0][1]..].chars().next().unwrap() == '\n')
            }
        }
    }

    pub(crate) fn input(self, app: &mut App, io: &mut dyn IO, event: InputEvent<'_>) {
        let mut flush_doing = true;

        match event {
            InputEvent::Key {
                state, logical_key, ..
            } if state == ElementState::Pressed && app.modifiers.control && !app.modifiers.alt => {
                match logical_key {
                    Key::Character("i") => self.cursor_move(app, Direction::Up),
                    Key::Character("k") => self.cursor_move(app, Direction::Down),
                    Key::Character("j") => self.cursor_move(app, Direction::Left),
                    Key::Character("l") => self.cursor_move(app, Direction::Right),
                    Key::Named(NamedKey::Space) => self.toggle_mark(app),
                    Key::Character("d") => self.cursor_add_next_match(app),
                    Key::Character("D") => self.cursor_remove_last(app),
                    Key::Character("c") => self.cursor_copy(app, io),
                    Key::Character("x") => self.cursor_cut(app, io),
                    Key::Character("v") => self.cursor_paste(app, io),
                    Key::Character("V") => self.cursor_paste_many(app, io),
                    Key::Character("s") => {
                        self.get(app).document_id.save(app, io, SaveKind::Explicit)
                    }
                    Key::Character("z") => self.undo(app),
                    Key::Character("Z") => self.redo(app),
                    _ => flush_doing = false,
                }
            }
            InputEvent::Key {
                state, logical_key, ..
            } if state == ElementState::Pressed && !app.modifiers.control && app.modifiers.alt => {
                match logical_key {
                    Key::Character("j") => self.cursor_goto_line_start(app),
                    Key::Character("l") => self.cursor_goto_line_end(app),
                    Key::Character("i") => self.cursor_goto_doc_start(app),
                    Key::Character("k") => self.cursor_goto_doc_end(app),
                    _ => flush_doing = false,
                }
            }
            InputEvent::Key {
                state, logical_key, ..
            } if state == ElementState::Pressed && !app.modifiers.control && !app.modifiers.alt => {
                match logical_key {
                    Key::Character(char) => {
                        self.cursor_replace(app, char.into());
                        flush_doing = false;
                    }
                    Key::Named(NamedKey::Enter) => {
                        self.cursor_replace(app, "\n".into());
                        flush_doing = false;
                    }
                    Key::Named(NamedKey::Space) => {
                        self.cursor_replace(app, " ".into());
                        flush_doing = false;
                    }
                    Key::Named(NamedKey::Backspace) => {
                        self.cursor_delete_left(app);
                        flush_doing = false;
                    }
                    Key::Named(NamedKey::Delete) => {
                        self.cursor_delete_right(app);
                        flush_doing = false;
                    }
                    _ => flush_doing = false,
                }
            }
            InputEvent::MouseButton { state, position } => match state {
                ElementState::Pressed => self.cursor_begin_drag(app, position),
                ElementState::Released => self.get_mut(app).is_dragging = false,
            },
            InputEvent::MouseWheel { y_offset } => {
                self.get_mut(app).top_pixel -= (SCROLL_AMOUNT * y_offset) as isize;
            }
            InputEvent::FocusChanged { focused: false } => {
                self.get(app).document_id.save(app, io, SaveKind::Auto)
            }
            _ => flush_doing = false,
        }

        let frame_start = app.frame_start;
        let editor = self.get_mut(app);
        editor.last_input = frame_start;
        if flush_doing {
            editor.document_id.flush_doing(app);
        }
    }

    pub(crate) fn tick(self, app: &mut App, io: &mut dyn IO) {
        let document_id = self.get(app).document_id;
        document_id.tick(app, io);

        // During drag, poll mouse position and update cursor head.
        if self.get(app).is_dragging {
            // Scroll when mouse is off-screen vertically.
            let mouse_position = app.mouse_position;
            {
                let editor = self.get_mut(app);
                if mouse_position[1] < 0.0 {
                    editor.top_pixel -= SCROLL_AMOUNT as isize;
                } else if mouse_position[1] > editor.last_viewport_size[1] {
                    editor.top_pixel += SCROLL_AMOUNT as isize;
                }
            }
            self.clamp_top_pixel(app);

            let frame_start = app.frame_start;

            // Drag main cursor.
            let offset = self.offset_from_screen(app, mouse_position);
            let editor = self.get_mut(app);
            let cursor = editor.cursors.last_mut().unwrap();
            let moved = cursor.head.offset != offset;
            cursor.head = CursorPoint::new(offset);
            if moved {
                editor.marked = true;
            }

            editor.last_input = frame_start;
        }

        // Animate cursor.
        let frame_start = app.frame_start;
        let editor = self.get_mut(app);
        editor.show_cursor = (((frame_start - editor.last_input).as_millis() / 500) % 2) == 0;
    }

    pub(crate) fn draw(self, app: &mut App, drawing: &mut Drawing) {
        let viewport_size = drawing.size();
        let grid_w = app.grid_from_screen(viewport_size)[0] as usize;
        if grid_w <= 2 {
            // The screen is too small to draw anything.
            return;
        }
        // Leave space for gutters
        let wrap_chars = grid_w - 2;

        let center_before = self.center_offset(app);

        {
            let editor = self.get_mut(app);
            if editor.wrap_chars != wrap_chars {
                editor.wrap_chars = wrap_chars;
                self.refresh_wraps(app);
            }
        }

        {
            let editor = self.get_mut(app);
            if editor.last_viewport_size != viewport_size {
                editor.last_viewport_size = viewport_size;
                let center_before = center_before.min(editor.document_id.text(app).len());
                self.scroll_offset_into_center(app, center_before);
            }
        }

        self.clamp_top_pixel(app);

        self.get(app).document_id.get_mut(app).last_center_offset = self.center_offset(app);

        let editor = self.get(app);
        let translate_y = -editor.top_pixel as f32;

        // Compute the visible line range so we don't iterate the whole doc.
        let line_first = (app.grid_from_screen([0.0, editor.top_pixel as f32])[1].max(0) as usize)
            .min(editor.wraps.len());
        let line_after =
            (app.grid_from_screen([0.0, editor.top_pixel as f32 + viewport_size[1]])[1].max(0)
                as usize
                + 1)
            .min(editor.wraps.len());

        let gutter_w = app.screen_from_grid([1, 0])[0];

        // Left gutter: soft-wrap continuation markers.
        {
            let mut drawing = drawing.push_clip_rect(Rect {
                pos: [0.0, 0.0],
                size: [gutter_w, viewport_size[1]],
            });
            let text = &editor.document_id.text(app);
            for line_idx in line_first..line_after {
                let [start, _end] = editor.wraps[line_idx];
                if start > 0 && text[start - 1] != b'\n' {
                    let mut pos = app.screen_from_grid([0, line_idx]);
                    pos[1] += translate_y;
                    drawing.draw_text(app.cell_size(), BStr::new(b"\\"), pos, HIGHLIGHT_COLOR);
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
            let total_h = (app.screen_from_grid([0, editor.wraps.len()])[1]).max(viewport_h_f);
            let top_y = ((-translate_y) / total_h * viewport_h_f).clamp(0.0, viewport_h_f);
            let bot_y =
                (((-translate_y) + viewport_h_f) / total_h * viewport_h_f).clamp(0.0, viewport_h_f);
            let h = (bot_y - top_y).max(1.0);
            let gutter_size = drawing.size();
            drawing.draw_rect(
                Rect {
                    pos: [0.0, 0.0],
                    size: gutter_size,
                },
                HIGHLIGHT_COLOR,
            );
            drawing.draw_rect(
                Rect {
                    pos: [0.0, top_y],
                    size: [gutter_w, h],
                },
                BACKGROUND_COLOR,
            );
        }

        // Text region: marks, text, cursors.
        {
            let width = app.screen_from_grid([wrap_chars, 0])[0];
            let mut drawing = drawing.push_clip_rect(Rect {
                pos: app.screen_from_grid([1, 0]),
                size: [width, viewport_size[1]],
            });

            // Draw mark.
            if editor.marked {
                for cursor in &editor.cursors {
                    if editor.marked {
                        let range = cursor.range();
                        for line_idx in line_first..line_after {
                            let [wrap_start, wrap_end] = editor.wraps[line_idx];
                            if range.end <= wrap_start || range.start > wrap_end {
                                continue;
                            }
                            let mark_start = range.start.max(wrap_start);
                            let mark_end = range.end.min(wrap_end);
                            let grid_start = self.grid_from_offset(app, mark_start)[1];
                            let mut grid_end = self.grid_from_offset(app, mark_end)[0];
                            grid_end[1] += 1;
                            let mut screen_start = app.screen_from_grid(grid_start);
                            let mut screen_end = app.screen_from_grid(grid_end);
                            screen_start[1] += translate_y;
                            screen_end[1] += translate_y;
                            drawing.draw_rect(
                                Rect::from_corners(screen_start, screen_end),
                                HIGHLIGHT_COLOR,
                            );
                        }
                    }
                }
            }

            // Draw text.
            {
                let text = &editor.document_id.text(app);
                for line_idx in line_first..line_after {
                    let [start, end] = editor.wraps[line_idx];
                    let mut screen = app.screen_from_grid([0, line_idx]);
                    screen[1] += translate_y;
                    drawing.draw_text(
                        app.cell_size(),
                        &text.as_bstr()[start..end],
                        screen,
                        TEXT_COLOR,
                    );
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
                    for grid_start in self.grid_from_offset(app, cursor.head.offset) {
                        let mut grid_end = grid_start;
                        grid_end[1] += 1;
                        let mut screen_start = app.screen_from_grid(grid_start);
                        let mut screen_end = app.screen_from_grid(grid_end);
                        screen_start[1] += translate_y;
                        screen_end[1] += translate_y;
                        let w = app.screen_from_grid([1, 0])[0] / 8.0;
                        screen_start[0] -= w / 2.0;
                        screen_end[0] += w / 2.0;
                        drawing
                            .draw_rect(Rect::from_corners(screen_start, screen_end), cursor_color);
                    }
                }
            }
        }
    }

    pub(crate) fn handle_edits(self, app: &mut App, diff: &OffsetDiff) {
        let center_before = self.center_offset(app);
        let editor = self.get_mut(app);

        let mut cursors = replace(&mut editor.cursors, vec![]);
        for cursor in &mut cursors {
            for point in [&mut cursor.head, &mut cursor.tail] {
                *point = CursorPoint::new(diff.apply(point.offset));
            }
        }
        editor.cursors = cursors;
        self.refresh_wraps(app);

        self.scroll_offset_into_center(app, diff.apply(center_before));
    }

    fn scroll_offset_into_view(self, app: &mut App, offset: usize) {
        let viewport_h = self.get(app).last_viewport_size[1] as isize;
        if viewport_h <= 0 {
            return;
        }
        let line = self.grid_from_offset(app, offset)[1][1];
        let y = app.screen_from_grid([0, line])[1] as isize;
        let y_end = app.screen_from_grid([0, line + 1])[1] as isize;
        let editor = self.get_mut(app);
        if y < editor.top_pixel {
            editor.top_pixel = y;
        }
        if y_end > editor.top_pixel + viewport_h {
            editor.top_pixel = y_end - viewport_h;
        }
    }

    fn scroll_main_cursor_into_view(self, app: &mut App) {
        let offset = self.get(app).cursors.last().unwrap().head.offset;
        self.scroll_offset_into_view(app, offset);
    }

    fn scroll_offset_into_center(self, app: &mut App, offset: usize) {
        let viewport_h = self.get(app).last_viewport_size[1] as isize;
        if viewport_h <= 0 {
            return;
        }
        let line = self.grid_from_offset(app, offset)[1][1];
        let y = app.screen_from_grid([0, line])[1] as isize;
        let y_end = app.screen_from_grid([0, line + 1])[1] as isize;
        self.get_mut(app).top_pixel = (y + y_end) / 2 - viewport_h / 2;
    }

    fn center_offset(self, app: &App) -> usize {
        let editor = self.get(app);
        let viewport_h = editor.last_viewport_size[1] as isize;
        let center_y = editor.top_pixel + viewport_h / 2;
        let line = app.grid_from_screen([0.0, center_y as f32])[1].max(0) as usize;
        let line = line.min(editor.wraps.len() - 1);
        editor.wraps[line][0]
    }

    fn clamp_top_pixel(self, app: &mut App) {
        let total_h = {
            let editor = self.get(app);
            app.screen_from_grid([0, editor.wraps.len()])[1] as isize
        };
        let editor = self.get_mut(app);
        let viewport_h = editor.last_viewport_size[1] as isize;
        if viewport_h > 0 {
            let max_top = (total_h - viewport_h / 2).max(0);
            if editor.top_pixel > max_top {
                editor.top_pixel = max_top;
            }
        }
        if editor.top_pixel < 0 {
            editor.top_pixel = 0;
        }
    }

    fn offset_from_screen(self, app: &App, screen_pos: [f32; 2]) -> usize {
        let editor = self.get(app);
        let cell_w = app.cell_size()[0] as f32;
        let doc_y = screen_pos[1] + editor.top_pixel as f32;
        let grid = app.grid_from_screen([screen_pos[0], doc_y]);
        if grid[1] < 0 {
            return 0;
        }
        let line = grid[1] as usize;
        if line >= editor.wraps.len() {
            return editor.document_id.text(app).len();
        }
        let screen_col = grid[0].max(0) as usize;
        if screen_col == 0 {
            return editor.wraps[line][0];
        }

        let col = screen_col - 1;
        let sub_x = screen_pos[0] - (screen_col as f32) * cell_w;
        let col = if sub_x >= cell_w / 2.0 { col + 1 } else { col };

        let [wrap_start, wrap_end] = editor.wraps[line];
        let text = &editor.document_id.text(app);
        let text_span = &text[wrap_start..wrap_end];
        let mut char_idx = 0;
        for (char_start, _char_end, _) in text_span.char_indices() {
            if char_idx >= col {
                return wrap_start + char_start;
            }
            char_idx += 1;
        }
        wrap_end
    }

    fn toggle_mark(self, app: &mut App) {
        let editor = self.get_mut(app);
        if editor.marked {
            editor.marked = false;
        } else {
            editor.marked = true;
            for cursor in &mut editor.cursors {
                cursor.tail = cursor.head;
            }
        }
    }

    fn cursor_begin_drag(self, app: &mut App, position: [f32; 2]) {
        let offset = self.offset_from_screen(app, position);
        let control_key = app.modifiers.control;
        let editor = self.get_mut(app);

        // Ctrl-click / Ctrl-drag: add a new cursor
        // Click / drag: set main cursor, remove others
        if !control_key {
            editor.cursors.clear();
        }

        editor.cursors.push(Cursor {
            head: CursorPoint::new(offset),
            tail: CursorPoint::new(offset),
        });
        editor.is_dragging = true;
        editor.marked = false;
        self.scroll_main_cursor_into_view(app);
    }

    fn cursor_add_next_match(self, app: &mut App) {
        let editor = self.get(app);
        let text = editor.document_id.text(app).as_bstr();
        let cursor_main = editor.cursors.last().unwrap();
        let range = cursor_main.range();
        if !editor.marked || range.start == range.end {
            return;
        };
        let search_start = range.end;
        let search_text = &text[range];
        if let Some(offset) = text[search_start..].find(search_text.as_bstr()) {
            let start = search_start + offset;
            let end = start + search_text.len();
            let mut cursor_new = Cursor {
                head: CursorPoint::new(end),
                tail: CursorPoint::new(start),
            };
            if cursor_main.head.offset < cursor_main.tail.offset {
                swap(&mut cursor_new.head, &mut cursor_new.tail);
            }
            let editor = self.get_mut(app);
            editor.cursors.push(cursor_new);
        }
    }

    fn cursor_remove_last(self, app: &mut App) {
        let editor = self.get_mut(app);
        if editor.cursors.len() > 1 {
            editor.cursors.pop();
        }
    }

    fn cursor_replace(self, app: &mut App, insert: &BStr) {
        self.cursor_replace_each(app, &|_| Some(insert))
    }

    fn cursor_replace_each<'a, F>(self, app: &mut App, insert_for_ix: &'a F)
    where
        F: Fn(usize) -> Option<&'a BStr>,
    {
        let mut cursors = take(&mut self.get_mut(app).cursors);
        let text = self.get(app).document_id.text(app).as_bstr();
        let mut edits = Vec::with_capacity(cursors.len() * 2);
        for (i, cursor) in cursors.iter_mut().enumerate() {
            let Some(insert) = insert_for_ix(i) else {
                continue;
            };
            if self.get(app).marked {
                let range = cursor.range();
                edits.push(Edit {
                    kind: EditKind::Insert,
                    offset: range.start,
                    text: insert.into(),
                });
                edits.push(Edit {
                    kind: EditKind::Delete,
                    offset: range.start,
                    text: text[range.start..range.end].into(),
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
        self.get_mut(app).cursors = cursors;
        self.get(app).document_id.apply_edits(app, &edits);
        self.get_mut(app).marked = false;
        self.scroll_main_cursor_into_view(app);
    }

    fn cursor_delete_left(self, app: &mut App) {
        let editor = self.get(app);
        let document_id = editor.document_id;
        let text = document_id.text(app);
        let mut edits = Vec::with_capacity(editor.cursors.len());
        for cursor in &editor.cursors {
            if editor.marked {
                let range = cursor.range();
                edits.push(Edit {
                    kind: EditKind::Delete,
                    offset: range.start,
                    text: text[range.start..range.end].into(),
                });
            } else if let Some(start) = document_id.char_prev(app, cursor.head.offset) {
                edits.push(Edit {
                    kind: EditKind::Delete,
                    offset: start,
                    text: text[start..cursor.head.offset].into(),
                });
            }
        }
        Edit::coalesce(&mut edits);
        document_id.apply_edits(app, &edits);
        let editor = self.get_mut(app);
        editor.marked = false;
        self.scroll_main_cursor_into_view(app);
    }

    fn cursor_delete_right(self, app: &mut App) {
        let editor = self.get(app);
        let document_id = editor.document_id;
        let text = document_id.text(app);
        let mut edits = Vec::with_capacity(editor.cursors.len());
        for cursor in &editor.cursors {
            if editor.marked {
                let range = cursor.range();
                edits.push(Edit {
                    kind: EditKind::Delete,
                    offset: range.start,
                    text: text[range.start..range.end].into(),
                });
            } else if let Some(end) = document_id.char_next(app, cursor.head.offset) {
                edits.push(Edit {
                    kind: EditKind::Delete,
                    offset: cursor.head.offset,
                    text: text[cursor.head.offset..end].into(),
                });
            }
        }
        Edit::coalesce(&mut edits);
        document_id.apply_edits(app, &edits);
        let editor = self.get_mut(app);
        editor.marked = false;
        self.scroll_main_cursor_into_view(app);
    }

    fn cursor_copy(self, app: &App, io: &mut dyn IO) {
        let editor = self.get(app);
        if !editor.marked {
            return;
        }
        let text = editor.document_id.text(app);
        let text = bstr::join(
            "\n",
            editor.cursors.iter().map(|cursor| &text[cursor.range()]),
        );
        io.set_clipboard_text(text.into());
    }

    fn cursor_cut(self, app: &mut App, io: &mut dyn IO) {
        self.cursor_copy(app, io);

        let editor = self.get(app);
        if !editor.marked {
            return;
        }

        self.cursor_replace(app, "".into());
    }

    fn cursor_paste(self, app: &mut App, io: &mut dyn IO) {
        let Some(text) = io.get_clipboard_text() else {
            return;
        };
        self.cursor_replace(app, text.as_bstr());
    }

    fn cursor_paste_many(self, app: &mut App, io: &mut dyn IO) {
        let Some(clip_text) = io.get_clipboard_text() else {
            return;
        };
        let lines: Vec<&BStr> = clip_text
            .split(|c| *c == b'\n')
            .map(|bs| bs.as_bstr())
            .collect();
        self.cursor_replace_each(app, &|i| lines.get(i).map(|s| *s))
    }

    fn refresh_wraps(self, app: &mut App) {
        let editor = self.get(app);
        self.get_mut(app).wraps =
            wraps_from_text(editor.document_id.text(app).as_bstr(), editor.wrap_chars);
    }

    fn cursor_goto_line_start(self, app: &mut App) {
        let document_id = self.get(app).document_id;
        let mut cursors = take(&mut self.get_mut(app).cursors);
        for cursor in &mut cursors {
            cursor.head = CursorPoint::new(
                document_id
                    .line_range_from_offset(app, cursor.head.offset)
                    .start,
            );
        }
        self.get_mut(app).cursors = cursors;
        self.scroll_main_cursor_into_view(app);
    }

    fn cursor_goto_line_end(self, app: &mut App) {
        let document_id = self.get(app).document_id;
        let mut cursors = take(&mut self.get_mut(app).cursors);
        for cursor in &mut cursors {
            cursor.head = CursorPoint::new(
                document_id
                    .line_range_from_offset(app, cursor.head.offset)
                    .end,
            );
        }
        self.get_mut(app).cursors = cursors;
        self.scroll_main_cursor_into_view(app);
    }

    fn cursor_goto_doc_start(self, app: &mut App) {
        let editor = self.get_mut(app);
        for cursor in &mut editor.cursors {
            cursor.head = CursorPoint::new(0);
        }
        self.scroll_offset_into_view(app, 0);
    }

    fn cursor_goto_doc_end(self, app: &mut App) {
        let end = self.get(app).document_id.text(app).len();
        let editor = self.get_mut(app);
        for cursor in &mut editor.cursors {
            cursor.head = CursorPoint::new(end);
        }
        self.scroll_offset_into_center(app, end);
    }

    fn cursor_move(self, app: &mut App, direction: Direction) {
        let document_id = self.get(app).document_id;
        let mut cursors = take(&mut self.get_mut(app).cursors);
        for cursor in &mut cursors {
            match direction {
                Direction::Left => {
                    cursor.head = CursorPoint::new(
                        document_id
                            .char_prev(app, cursor.head.offset)
                            .unwrap_or(cursor.head.offset),
                    );
                }
                Direction::Right => {
                    cursor.head = CursorPoint::new(
                        document_id
                            .char_next(app, cursor.head.offset)
                            .unwrap_or(cursor.head.offset),
                    );
                }
                Direction::Up => {
                    cursor.head = self.line_up(app, cursor.head).unwrap_or(cursor.head);
                }
                Direction::Down => {
                    cursor.head = self.line_down(app, cursor.head).unwrap_or(cursor.head);
                }
            };
        }
        self.get_mut(app).cursors = cursors;
        self.scroll_main_cursor_into_view(app);
    }

    // Return the grid position for a byte offset within the doc.
    // When the cursor is at a soft-wrap position there are two possible grid positions:
    // * at the end of one soft-wrapped line
    // * at the start of the next soft-wrapped line
    // This function returns both.
    // If the position is not ambiguous then both returned positions are equal.
    fn grid_from_offset(self, app: &App, offset: usize) -> [[usize; 2]; 2] {
        let editor = self.get(app);
        let text = editor.document_id.text(app).as_bstr();
        let line = editor
            .wraps
            .partition_point(|&[start, _end]| start <= offset)
            - 1;
        let grid1 = {
            let [start, end] = editor.wraps[line];
            assert!(offset <= end);
            let col = text[start..offset].chars().count();
            [col, line]
        };
        let grid0 = if line > 0 && editor.wraps[line - 1][1] == offset {
            let [start, end] = editor.wraps[line - 1];
            assert!(offset == end);
            let col = text[start..offset].chars().count();
            [col, line - 1]
        } else {
            grid1
        };
        [grid0, grid1]
    }

    fn line_up(self, app: &App, point: CursorPoint) -> Option<CursorPoint> {
        let editor = self.get(app);
        let text = editor.document_id.text(app).as_bstr();
        let line = self.grid_from_offset(app, point.offset)[0][1];
        if line == 0 {
            return None;
        }
        let col = point
            .col_wanted
            .unwrap_or(text[editor.wraps[line][0]..point.offset].chars().count());
        let wrap_prev = editor.wraps[line - 1];
        let mut result_offset = wrap_prev[0];
        if let Some((_, char_end, _)) = text[wrap_prev[0]..wrap_prev[1]]
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

    fn line_down(self, app: &App, point: CursorPoint) -> Option<CursorPoint> {
        let editor = self.get(app);
        let text = editor.document_id.text(app).as_bstr();
        let line = self.grid_from_offset(app, point.offset)[0][1];
        if line == editor.wraps.len() - 1 {
            return None;
        }
        let col = point
            .col_wanted
            .unwrap_or(text[editor.wraps[line][0]..point.offset].chars().count());
        let wrap_next = editor.wraps[line + 1];
        let mut result_offset = wrap_next[0];
        if let Some((_, char_end, _)) = text[wrap_next[0]..wrap_next[1]]
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

    fn undo(self, app: &mut App) {
        let document_id = self.get(app).document_id;
        if let Some(offset) = document_id.undo(app) {
            self.scroll_offset_into_center(app, offset);
        }
    }

    fn redo(self, app: &mut App) {
        let document_id = self.get(app).document_id;
        if let Some(offset) = document_id.redo(app) {
            self.scroll_offset_into_center(app, offset);
        }
    }
}

impl Cursor {
    fn range(&self) -> Range<usize> {
        if self.head.offset < self.tail.offset {
            self.head.offset..self.tail.offset
        } else {
            self.tail.offset..self.head.offset
        }
    }
}

impl CursorPoint {
    fn new(offset: usize) -> CursorPoint {
        CursorPoint {
            offset,
            col_wanted: None,
        }
    }
}

fn wraps_from_text(text: &BStr, wrap_chars: usize) -> Vec<[usize; 2]> {
    let mut wraps = vec![];
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

    wraps
}
