use std::mem::{swap, take};
use std::ops::Range;
use std::time::Duration;

use bstr::{BStr, ByteSlice};

use crate::input::{ButtonState, InputEvent, Key, NamedKey};
use crate::style::BACKGROUND_COLOR;
use crate::{
    app::{App, IO},
    buffer::{self, BufferId, Edit, EditKind, OffsetDiff, SaveKind},
    drawing::{Drawing, Rect},
    map::Map,
    style::{HIGHLIGHT_COLOR, MULTI_CURSOR_COLOR, TEXT_COLOR},
};

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy, Debug)]
pub struct EditorId(pub(crate) usize);

pub struct Editors {
    pub(crate) editor_count: usize,

    pub(crate) buffer_id: Map<EditorId, BufferId>,
    pub(crate) cursors: Map<EditorId, Vec<Cursor>>,
    // Draw a '>' in the left gutter next to the main cursor's line (used
    // by the FileOpen list to show the selected entry).
    pub(crate) gutter_marker: Map<EditorId, bool>,
    marked: Map<EditorId, bool>,
    show_cursor: Map<EditorId, bool>,
    wrap_chars: Map<EditorId, usize>,
    wraps: Map<EditorId, Vec<[usize; 2]>>,
    last_input: Map<EditorId, Duration>,
    top_pixel: Map<EditorId, isize>,
    last_draw_size: Map<EditorId, [f32; 2]>,
    last_mouse_position: Map<EditorId, [f32; 2]>,
    is_dragging: Map<EditorId, bool>,
}

const SCROLL_AMOUNT: f32 = 32.0;

#[derive(Clone)]
pub(crate) struct Cursor {
    pub(crate) head: CursorPoint,
    pub(crate) tail: CursorPoint,
}

#[derive(Copy, Clone)]
pub(crate) struct CursorPoint {
    pub(crate) offset: usize,
    // The column the cursor 'wants' to be at when moving up/down, if any.
    pub(crate) col_wanted: Option<usize>,
}

enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl Editors {
    pub(crate) fn new() -> Editors {
        Editors {
            editor_count: 0,
            buffer_id: Map::new(),
            cursors: Map::new(),
            gutter_marker: Map::new(),
            marked: Map::new(),
            show_cursor: Map::new(),
            wrap_chars: Map::new(),
            wraps: Map::new(),
            last_input: Map::new(),
            top_pixel: Map::new(),
            last_draw_size: Map::new(),
            last_mouse_position: Map::new(),
            is_dragging: Map::new(),
        }
    }
}

pub(crate) fn new(app: &mut App, buffer_id: BufferId) -> EditorId {
    let wrap_chars = 80;
    let wraps = wraps_from_text(buffer_id.text(app).as_bstr(), wrap_chars);
    let editor_id = EditorId(app.editors.editor_count);
    app.editors.editor_count += 1;
    app.editors.buffer_id.insert(editor_id, buffer_id);
    app.editors.cursors.insert(
        editor_id,
        vec![Cursor {
            head: CursorPoint::new(0),
            tail: CursorPoint::new(0),
        }],
    );
    app.editors.gutter_marker.insert(editor_id, false);
    app.editors.marked.insert(editor_id, false);
    app.editors.show_cursor.insert(editor_id, true);
    app.editors.wrap_chars.insert(editor_id, wrap_chars);
    app.editors.wraps.insert(editor_id, wraps);
    app.editors.last_input.insert(editor_id, Duration::ZERO);
    app.editors.top_pixel.insert(editor_id, 0);
    app.editors.last_draw_size.insert(editor_id, [0.0, 0.0]);
    app.editors
        .last_mouse_position
        .insert(editor_id, [0.0, 0.0]);
    app.editors.is_dragging.insert(editor_id, false);
    editor_id
}

pub(crate) fn new_scratch(app: &mut App) -> EditorId {
    let buffer_id = buffer::scratch(app);
    new(app, buffer_id)
}

pub(crate) fn new_generated(app: &mut App) -> EditorId {
    let buffer_id = buffer::generated(app);
    new(app, buffer_id)
}

/// A new editor over `buffer_id`, showing it exactly as `editor_id` does:
/// same cursors, selection, gutter marker and scroll position. `buffer_id`
/// must hold the same text as `editor_id`'s buffer, or the copied cursors
/// would be out of bounds.
pub(crate) fn new_like(app: &mut App, editor_id: EditorId, buffer_id: BufferId) -> EditorId {
    assert_eq!(
        app.editors.buffer_id[editor_id].text(app),
        buffer_id.text(app),
        "new_like needs a buffer holding the same text"
    );
    let copy_id = new(app, buffer_id);
    app.editors.cursors[copy_id] = app.editors.cursors[editor_id].clone();
    app.editors.marked[copy_id] = app.editors.marked[editor_id];
    app.editors.gutter_marker[copy_id] = app.editors.gutter_marker[editor_id];
    app.editors.wrap_chars[copy_id] = app.editors.wrap_chars[editor_id];
    app.editors.top_pixel[copy_id] = app.editors.top_pixel[editor_id];
    copy_id.refresh_wraps(app);
    copy_id
}

/// A new editor over a new buffer of the same kind holding a copy of
/// `editor_id`'s text, shown the same way. Used to duplicate the editors a
/// page owns; editors over buffers the page merely holds - a file, or
/// another page's buffer - share those with `new_like` instead.
pub(crate) fn new_copy(app: &mut App, editor_id: EditorId) -> EditorId {
    let buffer_id = buffer::copy(app, app.editors.buffer_id[editor_id]);
    new_like(app, editor_id, buffer_id)
}

pub(crate) fn assert_invariants(app: &App) {
    let editors = &app.editors;
    assert_eq!(editors.buffer_id.len(), editors.editor_count);
    assert_eq!(editors.cursors.len(), editors.editor_count);
    assert_eq!(editors.gutter_marker.len(), editors.editor_count);
    assert_eq!(editors.marked.len(), editors.editor_count);
    assert_eq!(editors.show_cursor.len(), editors.editor_count);
    assert_eq!(editors.wrap_chars.len(), editors.editor_count);
    assert_eq!(editors.wraps.len(), editors.editor_count);
    assert_eq!(editors.last_input.len(), editors.editor_count);
    assert_eq!(editors.top_pixel.len(), editors.editor_count);
    assert_eq!(editors.last_draw_size.len(), editors.editor_count);
    assert_eq!(editors.last_mouse_position.len(), editors.editor_count);
    assert_eq!(editors.is_dragging.len(), editors.editor_count);
    for editor_id in (0..editors.editor_count).map(EditorId) {
        assert!(app.editors.buffer_id[editor_id].0 < app.buffers.buffer_count);
        editor_id.assert_invariants(app);
    }
}

impl EditorId {
    fn assert_invariants(self, app: &App) {
        let buffer_id = app.editors.buffer_id[self];
        let cursors = &app.editors.cursors[self];
        let wrap_chars = app.editors.wrap_chars[self];
        let wraps = &app.editors.wraps[self];
        let text = buffer_id.text(app);

        // Cursors
        assert!(cursors.len() > 0);
        for cursor in cursors {
            for point in [&cursor.head, &cursor.tail] {
                assert!(point.offset <= text.len());
            }
        }

        // Wraps: incremental updates must match a full recompute.
        assert_eq!(*wraps, wraps_from_text(text, wrap_chars));
        assert!(!wraps.is_empty());
        assert_eq!(wraps[0][0], 0);
        assert_eq!(wraps.last().unwrap()[1], text.len());
        for wrap in wraps {
            assert!(wrap[0] <= wrap[1]);
            assert!(text[wrap[0]..wrap[1]].chars().count() <= wrap_chars)
        }
        for pair in wraps.windows(2) {
            let gap = pair[1][0] - pair[0][1];
            assert!(gap <= 1);
            if gap == 1 {
                assert!(text[pair[0][1]..].chars().next().unwrap() == '\n')
            }
        }
    }

    pub(crate) fn set_buffer(self, app: &mut App, buffer_id: BufferId) {
        if app.editors.buffer_id[self] == buffer_id {
            return;
        }
        app.editors.buffer_id[self] = buffer_id;
        self.refresh_wraps(app);
        self.cursor_reset(app);
    }

    pub(crate) fn main_cursor_offset(self, app: &App) -> usize {
        app.editors.cursors[self].last().unwrap().head.offset
    }

    pub(crate) fn set_cursor_offsets(self, app: &mut App, offsets: &[usize]) {
        if offsets.is_empty() {
            self.cursor_reset(app);
            return;
        }
        app.editors.cursors[self] = offsets
            .iter()
            .map(|&offset| Cursor {
                head: CursorPoint::new(offset),
                tail: CursorPoint::new(offset),
            })
            .collect();
        app.editors.marked[self] = false;
        self.scroll_main_cursor_into_view(app);
    }

    pub(crate) fn set_marked_ranges(self, app: &mut App, ranges: &[Range<usize>]) {
        if ranges.is_empty() {
            self.cursor_reset(app);
            return;
        }
        app.editors.cursors[self] = ranges
            .iter()
            .map(|range| Cursor {
                head: CursorPoint::new(range.end),
                tail: CursorPoint::new(range.start),
            })
            .collect();
        app.editors.marked[self] = true;
        self.scroll_main_cursor_into_view(app);
    }

    pub(crate) fn tick(self, app: &mut App, io: &mut dyn IO) {
        let buffer_id = app.editors.buffer_id[self];
        buffer_id.tick(app, io);

        // During drag, poll mouse position and update cursor head.
        if app.editors.is_dragging[self] {
            let position = app.editors.last_mouse_position[self];
            // Scroll when mouse is off-screen vertically.
            if position[1] <= 0.0 {
                app.editors.top_pixel[self] -= SCROLL_AMOUNT as isize;
            } else if position[1] >= app.editors.last_draw_size[self][1] {
                app.editors.top_pixel[self] += SCROLL_AMOUNT as isize;
            }
            self.clamp_top_pixel(app);

            // Drag main cursor.
            let offset = self.offset_from_screen(app, position);
            let cursors = &mut app.editors.cursors[self];
            let cursor = cursors.last_mut().unwrap();
            let moved = cursor.head.offset != offset;
            cursor.head = CursorPoint::new(offset);
            if moved {
                app.editors.marked[self] = true;
            }

            app.editors.last_input[self] = app.frame_start;
        }

        // Animate cursor.
        let since_input = app.frame_start - app.editors.last_input[self];
        app.editors.show_cursor[self] = ((since_input.as_millis() / 500) % 2) == 0;
    }

    pub(crate) fn input(self, app: &mut App, io: &mut dyn IO, event: InputEvent<'_>) {
        // Generated buffers - picker lists, previews, runner output, status
        // bars - are rewritten by their page every tick, so the keys that
        // would edit them fall through to the catch-all and are ignored.
        // Everything that only moves or reads - cursors, marking, copy,
        // mouse, scrolling - still works, because selecting a line of
        // command output and copying it is the point of showing it in an
        // editor at all. Read from the buffer rather than the editor: an
        // editor can be pointed at a different buffer (the open_buffer
        // preview switches between the selection and a placeholder).
        let editable = app.editors.buffer_id[self].is_editable(app);
        let mut flush_doing = true;

        match event {
            InputEvent::Key {
                state, logical_key, ..
            } if state == ButtonState::Pressed && app.modifiers.control && !app.modifiers.alt => {
                match logical_key {
                    Key::Character("i") => self.cursor_move(app, Direction::Up),
                    Key::Character("k") => self.cursor_move(app, Direction::Down),
                    Key::Character("j") => self.cursor_move(app, Direction::Left),
                    Key::Character("l") => self.cursor_move(app, Direction::Right),
                    Key::Named(NamedKey::Space) => self.toggle_mark(app),
                    Key::Character("d") => self.cursor_add_next_match(app),
                    Key::Character("D") => self.cursor_remove_last(app),
                    Key::Character("c") => self.cursor_copy(app, io),
                    Key::Character("x") if editable => self.cursor_cut(app, io),
                    Key::Character("v") if editable => self.cursor_paste(app, io),
                    Key::Character("V") if editable => self.cursor_paste_many(app, io),
                    Key::Character("s") if editable => {
                        let buffer_id = app.editors.buffer_id[self];
                        buffer_id.save(app, io, SaveKind::Explicit);
                    }
                    Key::Character("z") if editable => self.undo(app),
                    Key::Character("Z") if editable => self.redo(app),
                    _ => flush_doing = false,
                }
            }
            InputEvent::Key {
                state, logical_key, ..
            } if state == ButtonState::Pressed && !app.modifiers.control && app.modifiers.alt => {
                match logical_key {
                    Key::Character("j") => self.cursor_goto_line_start(app),
                    Key::Character("l") => self.cursor_goto_line_end(app),
                    Key::Character("i") => self.cursor_goto_buffer_start(app),
                    Key::Character("k") => self.cursor_goto_buffer_end(app),
                    _ => flush_doing = false,
                }
            }
            InputEvent::Key {
                state, logical_key, ..
            } if state == ButtonState::Pressed && !app.modifiers.control && !app.modifiers.alt => {
                match logical_key {
                    Key::Character(char) if editable => {
                        self.cursor_replace(app, char.into());
                        flush_doing = false;
                    }
                    Key::Named(NamedKey::Enter) if editable => {
                        self.cursor_replace(app, "\n".into());
                        flush_doing = false;
                    }
                    Key::Named(NamedKey::Space) if editable => {
                        self.cursor_replace(app, " ".into());
                        flush_doing = false;
                    }
                    Key::Named(NamedKey::Backspace) if editable => {
                        self.cursor_delete_left(app);
                        flush_doing = false;
                    }
                    Key::Named(NamedKey::Delete) if editable => {
                        self.cursor_delete_right(app);
                        flush_doing = false;
                    }
                    _ => flush_doing = false,
                }
            }
            InputEvent::MouseButton { state, position } => match state {
                ButtonState::Pressed => self.cursor_begin_drag(app, position),
                ButtonState::Released => app.editors.is_dragging[self] = false,
            },
            InputEvent::MouseWheel { y_offset } => {
                app.editors.top_pixel[self] -= (SCROLL_AMOUNT * y_offset) as isize;
            }
            InputEvent::MouseMoved { position } => {
                app.editors.last_mouse_position[self] = position;
            }
            InputEvent::FocusChanged { focused: false } if editable => {
                let buffer_id = app.editors.buffer_id[self];
                buffer_id.save(app, io, SaveKind::Auto);
            }
            _ => flush_doing = false,
        }

        app.editors.last_input[self] = app.frame_start;
        if flush_doing && editable {
            let buffer_id = app.editors.buffer_id[self];
            buffer_id.flush_doing(app);
        }
    }

    pub(crate) fn draw(self, app: &mut App, drawing: &mut Drawing, focused: bool) {
        let viewport_size = drawing.size();
        let grid_w = app.grid_from_screen(viewport_size)[0] as usize;
        if grid_w <= 2 {
            // The screen is too small to draw anything.
            return;
        }
        // Leave space for gutters
        let wrap_chars = grid_w - 2;

        let center_before = self.center_offset(app);

        if app.editors.wrap_chars[self] != wrap_chars {
            app.editors.wrap_chars[self] = wrap_chars;
            self.refresh_wraps(app);
        }

        if app.editors.last_draw_size[self] != viewport_size {
            app.editors.last_draw_size[self] = viewport_size;
            let buffer_id = app.editors.buffer_id[self];
            let center_before = center_before.min(buffer_id.text(app).len());
            self.scroll_offset_into_center(app, center_before);
        }

        self.clamp_top_pixel(app);

        let buffer_id = app.editors.buffer_id[self];
        app.buffers.last_center_offset[buffer_id] = self.center_offset(app);

        let cursors = &app.editors.cursors[self];
        let marked = app.editors.marked[self];
        let show_cursor = app.editors.show_cursor[self];
        let wraps = &app.editors.wraps[self];
        let top_pixel = app.editors.top_pixel[self];
        let translate_y = -top_pixel as f32;

        // Compute the visible line range so we don't iterate the whole buffer.
        let line_first =
            (app.grid_from_screen([0.0, top_pixel as f32])[1].max(0) as usize).min(wraps.len());
        let line_after =
            (app.grid_from_screen([0.0, top_pixel as f32 + viewport_size[1]])[1].max(0) as usize
                + 1)
            .min(wraps.len());

        let gutter_w = app.screen_from_grid([1, 0])[0];

        // Draw background.
        drawing.draw_rect(
            Rect {
                pos: [0.0, 0.0],
                size: drawing.size(),
            },
            BACKGROUND_COLOR,
        );

        // Left gutter: soft-wrap continuation markers.
        {
            let mut drawing = drawing.push_clip_rect(Rect {
                pos: [0.0, 0.0],
                size: [gutter_w, viewport_size[1]],
            });
            let text = &buffer_id.text(app);
            for line_idx in line_first..line_after {
                let [start, _end] = wraps[line_idx];
                if start > 0 && text[start - 1] != b'\n' {
                    let mut pos = app.screen_from_grid([0, line_idx]);
                    pos[1] += translate_y;
                    drawing.draw_text(app.cell_size(), BStr::new(b"\\"), pos, HIGHLIGHT_COLOR);
                }
            }

            // Marker next to the first row of the main cursor's line.
            if app.editors.gutter_marker[self] {
                let offset = cursors.last().unwrap().head.offset;
                let line_start = buffer_id.line_range_from_offset(app, offset).start;
                let row = self.grid_from_offset(app, line_start)[0][1];
                let mut pos = app.screen_from_grid([0, row]);
                pos[1] += translate_y;
                drawing.draw_text(app.cell_size(), BStr::new(b">"), pos, HIGHLIGHT_COLOR);
            }
        }

        // Right gutter: viewport-indicator rect.
        {
            let mut drawing = drawing.push_clip_rect(Rect {
                pos: [viewport_size[0] - gutter_w, 0.0],
                size: [gutter_w, viewport_size[1]],
            });
            let viewport_h_f = viewport_size[1];
            let total_h = (app.screen_from_grid([0, wraps.len()])[1]).max(viewport_h_f);
            let top_y = ((-translate_y) / total_h * viewport_h_f).clamp(0.0, viewport_h_f);
            let bot_y =
                (((-translate_y) + viewport_h_f) / total_h * viewport_h_f).clamp(0.0, viewport_h_f);
            drawing.draw_rect(
                Rect {
                    pos: [0.0, 0.0],
                    size: [gutter_w, top_y],
                },
                HIGHLIGHT_COLOR,
            );
            drawing.draw_rect(
                Rect {
                    pos: [0.0, bot_y],
                    size: [gutter_w, viewport_size[1] - bot_y],
                },
                HIGHLIGHT_COLOR,
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
            if marked {
                for cursor in cursors {
                    let range = cursor.range();
                    for line_idx in line_first..line_after {
                        let [wrap_start, wrap_end] = wraps[line_idx];
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

            // Draw text.
            {
                let text = &buffer_id.text(app);
                for line_idx in line_first..line_after {
                    let [start, end] = wraps[line_idx];
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
            if focused && show_cursor {
                let cursor_color = if cursors.len() > 1 {
                    MULTI_CURSOR_COLOR
                } else {
                    TEXT_COLOR
                };
                for cursor in cursors {
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

        let mut cursors = take(&mut app.editors.cursors[self]);
        for cursor in &mut cursors {
            for point in [&mut cursor.head, &mut cursor.tail] {
                *point = CursorPoint::new(diff.apply(point.offset));
            }
        }
        app.editors.cursors[self] = cursors;
        self.refresh_wraps_from(app, diff.unchanged_before());

        if app.editors.last_draw_size[self][1] > 0.0 {
            self.scroll_offset_into_center(app, diff.apply(center_before));
        }
    }

    fn scroll_offset_into_view(self, app: &mut App, offset: usize) {
        let viewport_h = app.editors.last_draw_size[self][1] as isize;
        if viewport_h <= 0 {
            return;
        }
        let line = self.grid_from_offset(app, offset)[1][1];
        let y = app.screen_from_grid([0, line])[1] as isize;
        let y_end = app.screen_from_grid([0, line + 1])[1] as isize;
        let top_pixel = &mut app.editors.top_pixel[self];
        if y < *top_pixel {
            *top_pixel = y;
        }
        if y_end > *top_pixel + viewport_h {
            *top_pixel = y_end - viewport_h;
        }
    }

    fn scroll_main_cursor_into_view(self, app: &mut App) {
        let offset = app.editors.cursors[self].last().unwrap().head.offset;
        self.scroll_offset_into_view(app, offset);
    }

    pub(crate) fn scroll_offset_into_center(self, app: &mut App, offset: usize) {
        let viewport_h = app.editors.last_draw_size[self][1] as isize;
        let line = self.grid_from_offset(app, offset)[1][1];
        if viewport_h <= 0 {
            app.editors.top_pixel[self] = app.screen_from_grid([0, line])[1] as isize;
            return;
        }
        let y = app.screen_from_grid([0, line])[1] as isize;
        let y_end = app.screen_from_grid([0, line + 1])[1] as isize;
        app.editors.top_pixel[self] = (y + y_end) / 2 - viewport_h / 2;
    }

    fn center_offset(self, app: &App) -> usize {
        let viewport_h = app.editors.last_draw_size[self][1] as isize;
        let center_y = app.editors.top_pixel[self] + viewport_h / 2;
        let line = app.grid_from_screen([0.0, center_y as f32])[1].max(0) as usize;
        let wraps = &app.editors.wraps[self];
        let line = line.min(wraps.len() - 1);
        wraps[line][0]
    }

    fn clamp_top_pixel(self, app: &mut App) {
        let total_h = app.screen_from_grid([0, app.editors.wraps[self].len()])[1] as isize;
        let viewport_h = app.editors.last_draw_size[self][1] as isize;
        let top_pixel = &mut app.editors.top_pixel[self];
        if viewport_h > 0 {
            let max_top = (total_h - viewport_h / 2).max(0);
            if *top_pixel > max_top {
                *top_pixel = max_top;
            }
        }
        if *top_pixel < 0 {
            *top_pixel = 0;
        }
    }

    fn offset_from_screen(self, app: &App, screen_pos: [f32; 2]) -> usize {
        let buffer_id = app.editors.buffer_id[self];
        let wraps = &app.editors.wraps[self];
        let top_pixel = app.editors.top_pixel[self];
        let cell_w = app.cell_size()[0] as f32;
        let doc_y = screen_pos[1] + top_pixel as f32;
        let grid = app.grid_from_screen([screen_pos[0], doc_y]);
        if grid[1] < 0 {
            return 0;
        }
        let line = grid[1] as usize;
        if line >= wraps.len() {
            return buffer_id.text(app).len();
        }
        let screen_col = grid[0].max(0) as usize;
        if screen_col == 0 {
            return wraps[line][0];
        }

        let col = screen_col - 1;
        let sub_x = screen_pos[0] - (screen_col as f32) * cell_w;
        let col = if sub_x >= cell_w / 2.0 { col + 1 } else { col };

        let [wrap_start, wrap_end] = wraps[line];
        let text = &buffer_id.text(app);
        match text[wrap_start..wrap_end].char_indices().nth(col) {
            Some((char_start, _, _)) => wrap_start + char_start,
            None => wrap_end,
        }
    }

    fn toggle_mark(self, app: &mut App) {
        let marked = !app.editors.marked[self];
        app.editors.marked[self] = marked;
        if marked {
            for cursor in &mut app.editors.cursors[self] {
                cursor.tail = cursor.head;
            }
        }
    }

    fn cursor_begin_drag(self, app: &mut App, position: [f32; 2]) {
        let offset = self.offset_from_screen(app, position);
        let control_key = app.modifiers.control;
        let cursors = &mut app.editors.cursors[self];

        // Ctrl-click / Ctrl-drag: add a new cursor
        // Click / drag: set main cursor, remove others
        if !control_key {
            cursors.clear();
        }

        cursors.push(Cursor {
            head: CursorPoint::new(offset),
            tail: CursorPoint::new(offset),
        });
        app.editors.is_dragging[self] = true;
        app.editors.marked[self] = false;
        self.scroll_main_cursor_into_view(app);
    }

    fn cursor_add_next_match(self, app: &mut App) {
        let buffer_id = app.editors.buffer_id[self];
        let cursor_main = app.editors.cursors[self].last().unwrap();
        let marked = app.editors.marked[self];
        let text = buffer_id.text(app).as_bstr();
        let range = cursor_main.range();
        if !marked || range.start == range.end {
            return;
        };
        let search_start = range.end;
        let search_text: Vec<u8> = text[range].as_bytes().to_vec();
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
            app.editors.cursors[self].push(cursor_new);
        }
    }

    fn cursor_remove_last(self, app: &mut App) {
        let cursors = &mut app.editors.cursors[self];
        if cursors.len() > 1 {
            cursors.pop();
        }
    }

    fn cursor_replace(self, app: &mut App, insert: &BStr) {
        self.cursor_replace_each(app, &|_| Some(insert))
    }

    fn cursor_replace_each<'a, F>(self, app: &mut App, insert_for_ix: &'a F)
    where
        F: Fn(usize) -> Option<&'a BStr>,
    {
        let buffer_id = app.editors.buffer_id[self];
        let marked = app.editors.marked[self];
        let mut cursors = take(&mut app.editors.cursors[self]);
        let text = buffer_id.text(app).as_bstr();
        let mut edits = Vec::with_capacity(cursors.len() * 2);
        for (i, cursor) in cursors.iter_mut().enumerate() {
            let Some(insert) = insert_for_ix(i) else {
                continue;
            };
            if marked {
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
        app.editors.cursors[self] = cursors;
        buffer_id.apply_edits(app, &edits);
        app.editors.marked[self] = false;
        self.scroll_main_cursor_into_view(app);
    }

    fn cursor_delete_left(self, app: &mut App) {
        let buffer_id = app.editors.buffer_id[self];
        let text = buffer_id.text(app);
        let cursors = &app.editors.cursors[self];
        let mut edits = Vec::with_capacity(cursors.len());
        for cursor in cursors {
            if app.editors.marked[self] {
                let range = cursor.range();
                edits.push(Edit {
                    kind: EditKind::Delete,
                    offset: range.start,
                    text: text[range.start..range.end].into(),
                });
            } else if let Some(start) = buffer_id.char_prev(app, cursor.head.offset) {
                edits.push(Edit {
                    kind: EditKind::Delete,
                    offset: start,
                    text: text[start..cursor.head.offset].into(),
                });
            }
        }
        Edit::coalesce(&mut edits);
        buffer_id.apply_edits(app, &edits);
        app.editors.marked[self] = false;
        self.scroll_main_cursor_into_view(app);
    }

    fn cursor_delete_right(self, app: &mut App) {
        let buffer_id = app.editors.buffer_id[self];
        let cursors = &app.editors.cursors[self];
        let marked = app.editors.marked[self];
        let text = buffer_id.text(app);
        let mut edits = Vec::with_capacity(cursors.len());
        for cursor in cursors {
            if marked {
                let range = cursor.range();
                edits.push(Edit {
                    kind: EditKind::Delete,
                    offset: range.start,
                    text: text[range.start..range.end].into(),
                });
            } else if let Some(end) = buffer_id.char_next(app, cursor.head.offset) {
                edits.push(Edit {
                    kind: EditKind::Delete,
                    offset: cursor.head.offset,
                    text: text[cursor.head.offset..end].into(),
                });
            }
        }
        Edit::coalesce(&mut edits);
        buffer_id.apply_edits(app, &edits);
        app.editors.marked[self] = false;
        self.scroll_main_cursor_into_view(app);
    }

    fn cursor_copy(self, app: &App, io: &mut dyn IO) {
        if !app.editors.marked[self] {
            return;
        }
        let buffer_id = app.editors.buffer_id[self];
        let cursors = &app.editors.cursors[self];
        let text = buffer_id.text(app);
        let text = bstr::join("\n", cursors.iter().map(|cursor| &text[cursor.range()]));
        io.set_clipboard_text(text.into());
    }

    fn cursor_cut(self, app: &mut App, io: &mut dyn IO) {
        self.cursor_copy(app, io);

        if !app.editors.marked[self] {
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
        self.cursor_replace_each(app, &|i| lines.get(i).copied())
    }

    fn refresh_wraps(self, app: &mut App) {
        let buffer_id = app.editors.buffer_id[self];
        let wrap_chars = app.editors.wrap_chars[self];
        app.editors.wraps[self] = wraps_from_text(buffer_id.text(app).as_bstr(), wrap_chars);
    }

    // Rewrap the text from `offset` onwards, leaving earlier wraps alone.
    //
    // A wrap is not decided by its own contents alone: to break at a space
    // the scan looks ahead up to one wrap width, so a wrap that ends before
    // `offset` can still have read past it and can still change. Only wraps
    // starting a whole width earlier are safe to keep, hence the margin.
    // A char is at most four bytes, and the scan reads one char past the
    // width before giving up, which is what the four and the plus one are.
    fn refresh_wraps_from(self, app: &mut App, offset: usize) {
        let buffer_id = app.editors.buffer_id[self];
        let wrap_chars = app.editors.wrap_chars[self];
        let margin = wrap_chars.saturating_add(1).saturating_mul(4);
        let safe = offset.saturating_sub(margin);
        // Wraps always start with [0, _], so there is one at or before any
        // offset and `keep` cannot underflow. Everything before it started
        // more than a wrap width before the edit, so it cannot have seen it.
        let keep = app.editors.wraps[self]
            .partition_point(|[start, _]| *start <= safe)
            .saturating_sub(1);
        let start = app.editors.wraps[self][keep][0];
        let mut tail = wraps_from_text(&buffer_id.text(app)[start..], wrap_chars);
        if start != 0 {
            for wrap in &mut tail {
                wrap[0] += start;
                wrap[1] += start;
            }
        }
        let wraps = &mut app.editors.wraps[self];
        // keep == 0 means start == 0, so the tail is already the whole
        // thing and can be moved in rather than copied.
        if keep == 0 {
            *wraps = tail;
        } else {
            wraps.truncate(keep);
            wraps.append(&mut tail);
        }
    }

    fn cursor_goto_line_start(self, app: &mut App) {
        let buffer_id = app.editors.buffer_id[self];
        let mut cursors = take(&mut app.editors.cursors[self]);
        for cursor in &mut cursors {
            cursor.head = CursorPoint::new(
                buffer_id
                    .line_range_from_offset(app, cursor.head.offset)
                    .start,
            );
        }
        app.editors.cursors[self] = cursors;
        self.scroll_main_cursor_into_view(app);
    }

    fn cursor_goto_line_end(self, app: &mut App) {
        let buffer_id = app.editors.buffer_id[self];
        let mut cursors = take(&mut app.editors.cursors[self]);
        for cursor in &mut cursors {
            cursor.head = CursorPoint::new(
                buffer_id
                    .line_range_from_offset(app, cursor.head.offset)
                    .end,
            );
        }
        app.editors.cursors[self] = cursors;
        self.scroll_main_cursor_into_view(app);
    }

    fn cursor_goto_buffer_start(self, app: &mut App) {
        for cursor in &mut app.editors.cursors[self] {
            cursor.head = CursorPoint::new(0);
        }
        self.scroll_offset_into_view(app, 0);
    }

    // Collapse to a single unmarked cursor at the buffer start and scroll
    // to the top.
    pub(crate) fn cursor_reset(self, app: &mut App) {
        app.editors.cursors[self] = vec![Cursor {
            head: CursorPoint::new(0),
            tail: CursorPoint::new(0),
        }];
        app.editors.marked[self] = false;
        self.scroll_main_cursor_into_view(app);
    }

    pub(crate) fn cursor_goto_buffer_end(self, app: &mut App) {
        let buffer_id = app.editors.buffer_id[self];
        let end = buffer_id.text(app).len();
        for cursor in &mut app.editors.cursors[self] {
            cursor.head = CursorPoint::new(end);
        }
        self.scroll_offset_into_center(app, end);
    }

    fn cursor_move(self, app: &mut App, direction: Direction) {
        let buffer_id = app.editors.buffer_id[self];
        let mut cursors = take(&mut app.editors.cursors[self]);
        for cursor in &mut cursors {
            match direction {
                Direction::Left => {
                    cursor.head = CursorPoint::new(
                        buffer_id
                            .char_prev(app, cursor.head.offset)
                            .unwrap_or(cursor.head.offset),
                    );
                }
                Direction::Right => {
                    cursor.head = CursorPoint::new(
                        buffer_id
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
        app.editors.cursors[self] = cursors;
        self.scroll_main_cursor_into_view(app);
    }

    // Return the grid position for a byte offset within the buffer.
    // When the cursor is at a soft-wrap position there are two possible grid positions:
    // * at the end of one soft-wrapped line
    // * at the start of the next soft-wrapped line
    // This function returns both.
    // If the position is not ambiguous then both returned positions are equal.
    pub(crate) fn grid_from_offset(self, app: &App, offset: usize) -> [[usize; 2]; 2] {
        let buffer_id = app.editors.buffer_id[self];
        let wraps = &app.editors.wraps[self];
        let text = buffer_id.text(app).as_bstr();
        let line = wraps.partition_point(|&[start, _end]| start <= offset) - 1;
        let grid1 = {
            let [start, end] = wraps[line];
            assert!(offset <= end);
            let col = text[start..offset].chars().count();
            [col, line]
        };
        let grid0 = if line > 0 && wraps[line - 1][1] == offset {
            let [start, end] = wraps[line - 1];
            assert!(offset == end);
            let col = text[start..offset].chars().count();
            [col, line - 1]
        } else {
            grid1
        };
        [grid0, grid1]
    }

    fn line_up(self, app: &App, point: CursorPoint) -> Option<CursorPoint> {
        let buffer_id = app.editors.buffer_id[self];
        let wraps = &app.editors.wraps[self];
        let text = buffer_id.text(app).as_bstr();
        let line = self.grid_from_offset(app, point.offset)[0][1];
        if line == 0 {
            return None;
        }
        let col = point
            .col_wanted
            .unwrap_or(text[wraps[line][0]..point.offset].chars().count());
        let wrap_prev = wraps[line - 1];
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
        let buffer_id = app.editors.buffer_id[self];
        let wraps = &app.editors.wraps[self];
        let text = buffer_id.text(app).as_bstr();
        let line = self.grid_from_offset(app, point.offset)[0][1];
        if line == wraps.len() - 1 {
            return None;
        }
        let col = point
            .col_wanted
            .unwrap_or(text[wraps[line][0]..point.offset].chars().count());
        let wrap_next = wraps[line + 1];
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
        let buffer_id = app.editors.buffer_id[self];
        if let Some(offset) = buffer_id.undo(app) {
            self.scroll_offset_into_center(app, offset);
        }
    }

    fn redo(self, app: &mut App) {
        let buffer_id = app.editors.buffer_id[self];
        if let Some(offset) = buffer_id.redo(app) {
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
