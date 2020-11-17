const focus = @import("../focus.zig");
usingnamespace focus.common;
const App = focus.App;
const Id = focus.Id;
const Buffer = focus.Buffer;
const LineWrappedBuffer = focus.LineWrappedBuffer;
const BufferSearcher = focus.BufferSearcher;
const Window = focus.Window;
const style = focus.style;
const meta = focus.meta;

pub const Point = struct {
    // what char we're at
    // 0 <= pos <= buffer.getBufferEnd()
    pos: usize,
    // what column we 'want' to be at
    // should only be updated by left/right movement
    // 0 <= col
    col: usize,
};

pub const Cursor = struct {
    // the actual cursor
    head: Point,
    // the other end of the selection, if editor.marked
    tail: Point,
};

pub const Dragging = enum {
    NotDragging,
    Dragging,
    CtrlDragging,
};

pub const Editor = struct {
    app: *App,
    buffer_id: Id,
    line_wrapped_buffer: LineWrappedBuffer,
    // cursors.len > 0
    cursors: ArrayList(Cursor),
    prev_main_cursor_head_pos: usize,
    marked: bool,
    dragging: Dragging,
    // which pixel of the buffer is at the top of the viewport
    top_pixel: isize,
    last_event_ms: i64,
    show_status_bar: bool,

    const scroll_amount = 32;

    pub fn init(app: *App, buffer_id: Id, show_status_bar: bool) Id {
        const self_buffer = app.getThing(buffer_id).Buffer;
        const line_wrapped_buffer = LineWrappedBuffer.init(app, self_buffer, std.math.maxInt(usize));
        var cursors = ArrayList(Cursor).init(app.allocator);
        cursors.append(.{
            .head = .{ .pos = 0, .col = 0 },
            .tail = .{ .pos = 0, .col = 0 },
        }) catch oom();
        const id = app.putThing(Editor{
            .app = app,
            .buffer_id = buffer_id,
            .line_wrapped_buffer = line_wrapped_buffer,
            .cursors = cursors,
            .prev_main_cursor_head_pos = 0,
            .marked = false,
            .dragging = .NotDragging,
            .top_pixel = 0,
            .last_event_ms = app.frame_time_ms,
            .show_status_bar = show_status_bar,
        });
        self_buffer.editor_ids.append(id) catch oom();
        return id;
    }

    pub fn deinit(self: *Editor) void {
        self.cursors.deinit();
        self.line_wrapped_buffer.deinit();
    }

    pub fn buffer(self: *Editor) *Buffer {
        return self.app.getThing(self.buffer_id).Buffer;
    }

    pub fn frame(self: *Editor, window: *Window, frame_rect: Rect, events: []const c.SDL_Event) void {
        var text_rect = frame_rect;
        const status_rect = if (self.show_status_bar) text_rect.splitBottom(self.app.atlas.char_height, 0) else null;
        const left_gutter_rect = text_rect.splitLeft(self.app.atlas.char_width, 0);
        const right_gutter_rect = text_rect.splitRight(self.app.atlas.char_width, 0);

        // window width might have changed
        const max_chars_per_line = @intCast(usize, @divTrunc(text_rect.w, self.app.atlas.char_width));
        if (self.line_wrapped_buffer.max_chars_per_line != max_chars_per_line) {
            self.line_wrapped_buffer.max_chars_per_line = max_chars_per_line;
            self.line_wrapped_buffer.update();
        }

        // TODO these state updates are total hacks
        // buffer might have been changed by another window
        self.correctInvalidCursors();

        // handle events
        // if we get textinput, we'll also get the keydown first
        // if the keydown is mapped to a command, we'll do that and ignore the textinput
        // TODO this assumes that they always arrive in the same frame, which the sdl docs are not clear about
        var accept_textinput = false;
        for (events) |event| {
            self.last_event_ms = self.app.frame_time_ms;
            switch (event.type) {
                c.SDL_KEYDOWN => {
                    const sym = event.key.keysym;
                    if (sym.mod == c.KMOD_LCTRL or sym.mod == c.KMOD_RCTRL) {
                        switch (sym.sym) {
                            ' ' => self.toggleMark(),
                            'c' => {
                                for (self.cursors.items) |*cursor| self.copy(cursor);
                                self.clearMark();
                            },
                            'x' => {
                                for (self.cursors.items) |*cursor| self.cut(cursor);
                                self.clearMark();
                            },
                            'v' => {
                                for (self.cursors.items) |*cursor| self.paste(cursor);
                                self.clearMark();
                            },
                            'j' => for (self.cursors.items) |*cursor| self.goLeft(cursor),
                            'l' => for (self.cursors.items) |*cursor| self.goRight(cursor),
                            'k' => for (self.cursors.items) |*cursor| self.goDown(cursor),
                            'i' => for (self.cursors.items) |*cursor| self.goUp(cursor),
                            'q' => {
                                self.collapseCursors();
                                self.clearMark();
                            },
                            'd' => self.addNextMatch(),
                            's' => self.save(),
                            'f' => {
                                const self_id = self.app.getId(self);
                                const selection = self.dupeSelection(self.app.frame_allocator, self.getMainCursor());
                                const buffer_searcher_id = BufferSearcher.init(self.app, self_id, selection);
                                window.pushView(buffer_searcher_id);
                            },
                            'z' => self.undo(),
                            // TODO ctrl+tab for indentall
                            else => accept_textinput = true,
                        }
                    } else if (sym.mod == c.KMOD_LCTRL | c.KMOD_LSHIFT or
                        sym.mod == c.KMOD_LCTRL | c.KMOD_RSHIFT or
                        sym.mod == c.KMOD_RCTRL | c.KMOD_LSHIFT or
                        sym.mod == c.KMOD_RCTRL | c.KMOD_RSHIFT)
                    {
                        switch (sym.sym) {
                            'z' => self.redo(),
                            else => accept_textinput = true,
                        }
                    } else if (sym.mod == c.KMOD_LALT or sym.mod == c.KMOD_RALT) {
                        switch (sym.sym) {
                            ' ' => for (self.cursors.items) |*cursor| self.swapHead(cursor),
                            'j' => for (self.cursors.items) |*cursor| self.goLineStart(cursor),
                            'l' => for (self.cursors.items) |*cursor| self.goLineEnd(cursor),
                            'k' => for (self.cursors.items) |*cursor| self.goBufferEnd(cursor),
                            'i' => for (self.cursors.items) |*cursor| self.goBufferStart(cursor),
                            else => accept_textinput = true,
                        }
                    } else if (sym.mod == 0) {
                        switch (sym.sym) {
                            c.SDLK_BACKSPACE => {
                                for (self.cursors.items) |*cursor| self.deleteBackwards(cursor);
                                self.clearMark();
                            },
                            c.SDLK_RETURN => {
                                for (self.cursors.items) |*cursor| {
                                    self.insert(cursor, &[1]u8{'\n'});
                                    self.indent(cursor);
                                }
                                self.clearMark();
                            },
                            c.SDLK_TAB => {
                                for (self.cursors.items) |*cursor| {
                                    self.indent(cursor);
                                }
                            },
                            // c.SDLK_RIGHT => for (self.cursors.items) |*cursor| self.goRight(cursor),
                            // c.SDLK_LEFT => for (self.cursors.items) |*cursor| self.goLeft(cursor),
                            // c.SDLK_DOWN => for (self.cursors.items) |*cursor| self.goDown(cursor),
                            // c.SDLK_UP => for (self.cursors.items) |*cursor| self.goUp(cursor),
                            c.SDLK_DELETE => {
                                for (self.cursors.items) |*cursor| self.deleteForwards(cursor);
                                self.clearMark();
                            },
                            else => accept_textinput = true,
                        }
                    } else {
                        accept_textinput = true;
                    }
                },
                c.SDL_TEXTINPUT => {
                    if (accept_textinput) {
                        const text = event.text.text[0..std.mem.indexOfScalar(u8, &event.text.text, 0).?];
                        for (self.cursors.items) |*cursor| self.insert(cursor, text);
                        self.clearMark();
                    }
                },
                c.SDL_MOUSEBUTTONDOWN => {
                    const button = event.button;
                    if (button.button == c.SDL_BUTTON_LEFT) {
                        const mouse_x = @intCast(Coord, button.x);
                        const mouse_y = @intCast(Coord, button.y);
                        if (text_rect.contains(mouse_x, mouse_y)) {
                            const line = @divTrunc(self.top_pixel + mouse_y - text_rect.y, self.app.atlas.char_height);
                            const col = @divTrunc(mouse_x - text_rect.x + @divTrunc(self.app.atlas.char_width, 2), self.app.atlas.char_width);
                            const pos = self.line_wrapped_buffer.getPosForLineCol(min(self.line_wrapped_buffer.countLines() - 1, @intCast(usize, max(line, 0))), @intCast(usize, max(col, 0)));
                            const mod = @enumToInt(c.SDL_GetModState());
                            if (mod == c.KMOD_LCTRL or mod == c.KMOD_RCTRL) {
                                self.dragging = .CtrlDragging;
                                var cursor = self.newCursor();
                                self.updatePos(&cursor.head, pos);
                                self.updatePos(&cursor.tail, pos);
                            } else {
                                self.dragging = .Dragging;
                                for (self.cursors.items) |*cursor| {
                                    self.updatePos(&cursor.head, pos);
                                    self.updatePos(&cursor.tail, pos);
                                }
                                self.clearMark();
                            }
                        }
                    }
                },
                c.SDL_MOUSEBUTTONUP => {
                    const button = event.button;
                    if (button.button == c.SDL_BUTTON_LEFT) {
                        self.dragging = .NotDragging;
                    }
                },
                c.SDL_MOUSEWHEEL => {
                    self.top_pixel -= scroll_amount * @intCast(i16, event.wheel.y);
                },
                else => {},
            }
        }

        // maybe start a new undo group
        if (self.app.frame_time_ms - self.last_event_ms > 500) {
            self.buffer().newUndoGroup();
        }

        // handle mouse drag
        // (might be dragging outside window, so can't rely on SDL_MOUSEMOTION
        if (self.dragging != .NotDragging) {
            // get mouse state
            var global_mouse_x: c_int = undefined;
            var global_mouse_y: c_int = undefined;
            const mouse_state = c.SDL_GetGlobalMouseState(&global_mouse_x, &global_mouse_y);
            var window_x: c_int = undefined;
            var window_y: c_int = undefined;
            c.SDL_GetWindowPosition(window.sdl_window, &window_x, &window_y);
            const mouse_x = @intCast(Coord, global_mouse_x - window_x);
            const mouse_y = @intCast(Coord, global_mouse_y - window_y);

            // update selection of dragged cursor
            const line = @divTrunc(self.top_pixel + mouse_y - text_rect.y, self.app.atlas.char_height);
            const col = @divTrunc(mouse_x - text_rect.x + @divTrunc(self.app.atlas.char_width, 2), self.app.atlas.char_width);
            const pos = self.line_wrapped_buffer.getPosForLineCol(min(self.line_wrapped_buffer.countLines() - 1, @intCast(usize, max(line, 0))), @intCast(usize, max(col, 0)));
            if (self.dragging == .CtrlDragging) {
                var cursor = &self.cursors.items[self.cursors.items.len - 1];
                if (cursor.tail.pos != pos and !self.marked) {
                    self.setMark();
                }
                self.updatePos(&cursor.head, pos);
            } else {
                for (self.cursors.items) |*cursor| {
                    if (cursor.tail.pos != pos and !self.marked) {
                        self.setMark();
                    }
                    self.updatePos(&cursor.head, pos);
                }
            }

            // if dragging outside window, scroll
            if (mouse_y <= text_rect.y) self.top_pixel -= scroll_amount;
            if (mouse_y >= text_rect.y + text_rect.h) self.top_pixel += scroll_amount;
        }

        // if cursor moved, scroll it into editor
        if (self.getMainCursor().head.pos != self.prev_main_cursor_head_pos) {
            self.prev_main_cursor_head_pos = self.getMainCursor().head.pos;
            const bottom_pixel = self.top_pixel + text_rect.h;
            const cursor_top_pixel = @intCast(isize, self.line_wrapped_buffer.getLineColForPos(self.getMainCursor().head.pos)[0]) * @intCast(isize, self.app.atlas.char_height);
            const cursor_bottom_pixel = cursor_top_pixel + @intCast(isize, self.app.atlas.char_height);
            if (cursor_top_pixel > bottom_pixel - @intCast(isize, self.app.atlas.char_height))
                self.top_pixel = cursor_top_pixel - @intCast(isize, text_rect.h) + @intCast(isize, self.app.atlas.char_height);
            if (cursor_bottom_pixel <= self.top_pixel + @intCast(isize, self.app.atlas.char_height))
                self.top_pixel = cursor_top_pixel;
        }

        // calculate visible range
        // ensure we don't scroll off the top or bottom of the buffer
        const max_pixels = @intCast(isize, self.line_wrapped_buffer.countLines()) * @intCast(isize, self.app.atlas.char_height);
        if (self.top_pixel < 0) self.top_pixel = 0;
        if (self.top_pixel > max_pixels) self.top_pixel = max_pixels;
        const num_visible_lines = @divTrunc(text_rect.h, self.app.atlas.char_height) + @rem(@rem(text_rect.h, self.app.atlas.char_height), 1); // round up
        const visible_start_line = @divTrunc(self.top_pixel, self.app.atlas.char_height); // round down
        const visible_end_line = visible_start_line + num_visible_lines;

        // draw background
        window.queueRect(text_rect, style.background_color);
        window.queueRect(left_gutter_rect, style.background_color);
        window.queueRect(right_gutter_rect, style.background_color);

        // can't draw if the window is too narrow
        if (max_chars_per_line == 0) return;

        // draw cursors, selections, text
        var line_ix = @intCast(usize, visible_start_line);
        const max_line_ix = min(@intCast(usize, visible_end_line + 1), self.line_wrapped_buffer.wrapped_line_ranges.items.len);
        while (line_ix < max_line_ix) : (line_ix += 1) {
            const line_range = self.line_wrapped_buffer.wrapped_line_ranges.items[line_ix];
            const line = self.buffer().bytes.items[line_range[0]..line_range[1]];

            const y = text_rect.y - @rem(self.top_pixel + 1, self.app.atlas.char_height) + ((@intCast(Coord, line_ix) - visible_start_line) * self.app.atlas.char_height);

            for (self.cursors.items) |cursor| {
                // draw cursor
                if (cursor.head.pos >= line_range[0] and cursor.head.pos <= line_range[1]) {
                    // blink
                    if (@mod(@divTrunc(self.app.frame_time_ms - self.last_event_ms, 500), 2) == 0) {
                        const x = text_rect.x + (@intCast(Coord, (cursor.head.pos - line_range[0])) * self.app.atlas.char_width);
                        const w = @divTrunc(self.app.atlas.char_width, 8);
                        window.queueRect(
                            .{
                                .x = @intCast(Coord, x) - @divTrunc(w, 2),
                                .y = @intCast(Coord, y),
                                .w = w,
                                .h = self.app.atlas.char_height,
                            },
                            if (self.cursors.items.len > 1) style.multi_cursor_color else style.text_color,
                        );
                    }
                }

                // draw selection
                if (self.marked) {
                    const selection_start_pos = min(cursor.head.pos, cursor.tail.pos);
                    const selection_end_pos = max(cursor.head.pos, cursor.tail.pos);
                    const highlight_start_pos = min(max(selection_start_pos, line_range[0]), line_range[1]);
                    const highlight_end_pos = min(max(selection_end_pos, line_range[0]), line_range[1]);
                    if ((highlight_start_pos < highlight_end_pos) or (selection_start_pos <= line_range[1] and selection_end_pos > line_range[1])) {
                        const x = text_rect.x + (@intCast(Coord, (highlight_start_pos - line_range[0])) * self.app.atlas.char_width);
                        const w = if (selection_end_pos > line_range[1])
                            text_rect.x + text_rect.w - x
                        else
                            @intCast(Coord, (highlight_end_pos - highlight_start_pos)) * self.app.atlas.char_width;
                        window.queueRect(.{
                            .x = @intCast(Coord, x),
                            .y = @intCast(Coord, y),
                            .w = @intCast(Coord, w),
                            .h = self.app.atlas.char_height,
                        }, style.highlight_color);
                    }
                }
            }

            // draw text
            window.queueText(.{ .x = text_rect.x, .y = @intCast(Coord, y), .w = text_rect.w, .h = text_rect.y + text_rect.h - @intCast(Coord, y) }, style.text_color, line);

            // if wrapped, draw arrows
            if (line_ix > 0 and line_range[0] == self.line_wrapped_buffer.wrapped_line_ranges.items[line_ix - 1][1]) {
                window.queueText(
                    .{ .x = left_gutter_rect.x, .y = @intCast(Coord, y), .h = self.app.atlas.char_height, .w = self.app.atlas.char_width },
                    style.highlight_color,
                    "\\",
                );
                // TODO need a font that has these arrows
                //window.queueQuad(
                //    .{ .x = left_gutter_rect.x, .y = @intCast(Coord, y), .h = self.app.atlas.char_height, .w = self.app.atlas.char_width },
                //    self.app.atlas.down_right_arrow_rect,
                //    style.highlight_color,
                //);
            }
            if (line_ix < self.line_wrapped_buffer.countLines() - 1 and line_range[1] == self.line_wrapped_buffer.wrapped_line_ranges.items[line_ix + 1][0]) {
                window.queueText(
                    .{ .x = right_gutter_rect.x, .y = @intCast(Coord, y), .h = self.app.atlas.char_height, .w = self.app.atlas.char_width },
                    style.highlight_color,
                    "\\",
                );
                // TODO need a font that has these arrows
                //window.queueQuad(
                //    .{ .x = right_gutter_rect.x, .y = @intCast(Coord, y), .h = self.app.atlas.char_height, .w = self.app.atlas.char_width },
                //    self.app.atlas.right_down_arrow_rect,
                //    style.highlight_color,
                //);
            }
        }

        // draw scrollbar
        {
            const ratio = @intToFloat(f64, self.top_pixel) / @intToFloat(f64, max_pixels);
            const y = text_rect.y + min(@floatToInt(Coord, @intToFloat(f64, text_rect.h) * ratio), text_rect.h - self.app.atlas.char_height);
            var left_scroll_rect = left_gutter_rect;
            left_scroll_rect.y = y;
            left_scroll_rect.h -= y;
            window.queueText(left_scroll_rect, style.highlight_color, ">");
            var right_scroll_rect = right_gutter_rect;
            right_scroll_rect.y = y;
            right_scroll_rect.h -= y;
            window.queueText(right_scroll_rect, style.highlight_color, "<");
        }

        // draw statusbar
        if (self.show_status_bar) {
            window.queueRect(status_rect.?, style.status_background_color);
            // use real line_col instead of wrapped
            const line_col = self.buffer().getLineColForPos(self.getMainCursor().head.pos);
            const filename = self.buffer().getFilename() orelse "";
            const status_text = format(self.app.frame_allocator, "{} L{} C{}", .{ filename, line_col[0], line_col[1] });
            window.queueText(status_rect.?, style.text_color, status_text);
        }
    }

    pub fn updateCol(self: *Editor, point: *Point) void {
        point.col = point.pos - self.line_wrapped_buffer.getLineStart(point.*.pos);
    }

    pub fn updatePos(self: *Editor, point: *Point, pos: usize) void {
        point.pos = pos;
        self.updateCol(point);
    }

    pub fn goPos(self: *Editor, cursor: *Cursor, pos: usize) void {
        self.updatePos(&cursor.head, pos);
    }

    pub fn goCol(self: *Editor, cursor: *Cursor, col: usize) void {
        const line_start = self.line_wrapped_buffer.getLineStart(cursor.head.pos);
        cursor.head.col = min(col, self.line_wrapped_buffer.getLineEnd(cursor.head.pos) - line_start);
        cursor.head.pos = line_start + cursor.head.col;
    }

    pub fn goLine(self: *Editor, cursor: *Cursor, line: usize) void {
        cursor.head.pos = self.line_wrapped_buffer.getPosForLine(line);
        // leave head.col intact
    }

    pub fn goLineCol(self: *Editor, cursor: *Cursor, line: usize, col: usize) void {
        self.goLine(cursor, line);
        self.goCol(cursor, col);
    }

    pub fn goLeft(self: *Editor, cursor: *Cursor) void {
        cursor.head.pos -= @as(usize, if (cursor.head.pos == 0) 0 else 1);
        self.updateCol(&cursor.head);
    }

    pub fn goRight(self: *Editor, cursor: *Cursor) void {
        cursor.head.pos += @as(usize, if (cursor.head.pos >= self.buffer().getBufferEnd()) 0 else 1);
        self.updateCol(&cursor.head);
    }

    pub fn goDown(self: *Editor, cursor: *Cursor) void {
        const line_col = self.line_wrapped_buffer.getLineColForPos(cursor.head.pos);
        if (line_col[0] < self.line_wrapped_buffer.countLines() - 1) {
            const col = cursor.head.col;
            cursor.head.pos = self.line_wrapped_buffer.getPosForLine(line_col[0] + 1);
            self.goCol(cursor, col);
            cursor.head.col = col;
        }
    }

    pub fn goUp(self: *Editor, cursor: *Cursor) void {
        const line_col = self.line_wrapped_buffer.getLineColForPos(cursor.head.pos);
        if (line_col[0] > 0) {
            const col = cursor.head.col;
            cursor.head.pos = self.line_wrapped_buffer.getPosForLine(line_col[0] - 1);
            self.goCol(cursor, col);
            cursor.head.col = col;
        }
    }

    pub fn goLineStart(self: *Editor, cursor: *Cursor) void {
        cursor.head.pos = self.line_wrapped_buffer.getLineStart(cursor.head.pos);
        cursor.head.col = 0;
    }

    pub fn goLineEnd(self: *Editor, cursor: *Cursor) void {
        cursor.head.pos = self.line_wrapped_buffer.getLineEnd(cursor.head.pos);
        self.updateCol(&cursor.head);
    }

    pub fn goBufferStart(self: *Editor, cursor: *Cursor) void {
        self.goPos(cursor, 0);
    }

    pub fn goBufferEnd(self: *Editor, cursor: *Cursor) void {
        self.goPos(cursor, self.buffer().getBufferEnd());
    }

    pub fn insert(self: *Editor, cursor: *Cursor, bytes: []const u8) void {
        self.deleteSelection(cursor);
        self.buffer().insert(cursor.head.pos, bytes);
        // buffer calls updateAfterInsert
    }

    pub fn updateAfterInsert(self: *Editor, start: usize, bytes: []const u8) void {
        self.line_wrapped_buffer.update();
        for (self.cursors.items) |*cursor| {
            for (&[2]*Point{ &cursor.head, &cursor.tail }) |point| {
                // TODO ptr compare is because we want paste to leave each cursor after its own insert
                // if (point.pos > insert_at or (point.pos == insert_at and @ptrToInt(other_cursor) >= @ptrToInt(cursor))) {
                if (point.pos >= start) {
                    point.pos += bytes.len;
                    self.updateCol(point);
                }
            }
        }
    }

    pub fn delete(self: *Editor, start: usize, end: usize) void {
        assert(start <= end);
        self.buffer().delete(start, end);
        // buffer calls updateAfterDelete
    }

    pub fn updateAfterDelete(self: *Editor, start: usize, end: usize) void {
        self.line_wrapped_buffer.update();
        for (self.cursors.items) |*cursor| {
            for (&[2]*Point{ &cursor.head, &cursor.tail }) |point| {
                if (point.pos >= start and point.pos <= end) point.pos = start;
                if (point.pos > end) point.pos -= (end - start);
                self.updateCol(point);
            }
        }
    }

    pub fn updateBeforeReplace(self: *Editor) [][2]usize {
        var line_cols = ArrayList([2]usize).init(self.app.frame_allocator);
        for (self.cursors.items) |cursor| {
            line_cols.append(self.buffer().getLineColForPos(cursor.head.pos)) catch oom();
        }
        return line_cols.toOwnedSlice();
    }

    pub fn updateAfterReplace(self: *Editor, line_cols: [][2]usize) void {
        self.line_wrapped_buffer.update();
        for (self.cursors.items) |*cursor, i| {
            const line_col = line_cols[i];
            self.goPos(cursor, self.buffer().getPosForLineCol(line_col[0], line_col[1]));
        }
    }

    pub fn deleteSelection(self: *Editor, cursor: *Cursor) void {
        if (self.marked) {
            const range = self.getSelectionRange(cursor);
            self.delete(range[0], range[1]);
        }
    }

    pub fn deleteBackwards(self: *Editor, cursor: *Cursor) void {
        if (self.marked) {
            self.deleteSelection(cursor);
        } else if (cursor.head.pos > 0) {
            self.delete(cursor.head.pos - 1, cursor.head.pos);
        }
    }

    pub fn deleteForwards(self: *Editor, cursor: *Cursor) void {
        if (self.marked) {
            self.deleteSelection(cursor);
        } else if (cursor.head.pos < self.buffer().getBufferEnd()) {
            self.delete(cursor.head.pos, cursor.head.pos + 1);
        }
    }

    pub fn clearMark(self: *Editor) void {
        self.marked = false;
    }

    pub fn setMark(self: *Editor) void {
        self.marked = true;
        for (self.cursors.items) |*cursor| {
            cursor.tail = cursor.head;
        }
    }

    pub fn toggleMark(self: *Editor) void {
        if (self.marked) {
            self.clearMark();
        } else {
            self.setMark();
        }
    }

    pub fn swapHead(self: *Editor, cursor: *Cursor) void {
        if (self.marked) {
            std.mem.swap(Point, &cursor.head, &cursor.tail);
        }
    }

    pub fn getSelectionRange(self: *Editor, cursor: *Cursor) [2]usize {
        if (self.marked) {
            const selection_start_pos = min(cursor.head.pos, cursor.tail.pos);
            const selection_end_pos = max(cursor.head.pos, cursor.tail.pos);
            return [2]usize{ selection_start_pos, selection_end_pos };
        } else {
            return [2]usize{ cursor.head.pos, cursor.head.pos };
        }
    }

    pub fn dupeSelection(self: *Editor, allocator: *Allocator, cursor: *Cursor) []const u8 {
        const range = self.getSelectionRange(cursor);
        return self.buffer().dupe(allocator, range[0], range[1]);
    }

    // TODO clipboard stack on app
    // TODO figure out nicer situation for multiple cursor copy/paste
    // could use wl-paste -w 'focus-copy'

    pub fn copy(self: *Editor, cursor: *Cursor) void {
        const text = self.dupeSelection(self.app.frame_allocator, cursor);
        const textZ = std.mem.dupeZ(self.app.frame_allocator, u8, text) catch oom();
        if (c.SDL_SetClipboardText(textZ) != 0) {
            panic("{s} while setting system clipboard", .{c.SDL_GetError()});
        }
    }

    pub fn cut(self: *Editor, cursor: *Cursor) void {
        self.copy(cursor);
        self.deleteSelection(cursor);
    }

    pub fn paste(self: *Editor, cursor: *Cursor) void {
        if (c.SDL_GetClipboardText()) |text| {
            defer c.SDL_free(text);
            self.insert(cursor, std.mem.spanZ(text));
        } else {
            warn("{s} while getting system clipboard", .{c.SDL_GetError()});
        }
    }

    // TODO rename to addCursor
    pub fn newCursor(self: *Editor) *Cursor {
        self.cursors.append(.{
            .head = .{ .pos = 0, .col = 0 },
            .tail = .{ .pos = 0, .col = 0 },
        }) catch oom();
        return &self.cursors.items[self.cursors.items.len - 1];
    }

    pub fn collapseCursors(self: *Editor) void {
        self.cursors.shrink(1);
    }

    pub fn getMainCursor(self: *Editor) *Cursor {
        return &self.cursors.items[self.cursors.items.len - 1];
    }

    pub fn addNextMatch(self: *Editor) void {
        const main_cursor = self.getMainCursor();
        const selection = self.dupeSelection(self.app.frame_allocator, main_cursor);
        if (self.buffer().searchForwards(max(main_cursor.head.pos, main_cursor.tail.pos), selection)) |pos| {
            var cursor = Cursor{
                .head = .{ .pos = pos + selection.len, .col = 0 },
                .tail = .{ .pos = pos, .col = 0 },
            };
            self.updateCol(&cursor.head);
            self.updateCol(&cursor.tail);
            self.cursors.append(cursor) catch oom();
        }
    }

    pub fn indent(self: *Editor, cursor: *Cursor) void {
        var self_buffer = self.buffer();

        // figure out how many lines we're going to indent _before_ we start changing them
        var num_lines: usize = 1;
        const range = self.getSelectionRange(cursor);
        {
            var pos = range[0];
            while (self_buffer.searchForwards(pos, "\n")) |new_pos| {
                if (new_pos >= range[1]) break;
                num_lines += 1;
                pos = new_pos + 1;
            }
        }

        // make a new cursor to peform the indents
        var edit_cursor = self.newCursor();
        std.mem.swap(Cursor, edit_cursor, &self.cursors.items[0]);
        edit_cursor = &self.cursors.items[0];
        defer {
            std.mem.swap(Cursor, edit_cursor, &self.cursors.items[self.cursors.items.len - 1]);
            _ = self.cursors.pop();
        }
        self.goPos(edit_cursor, range[0]);

        // for each line in selection
        while (num_lines > 0) : (num_lines -= 1) {

            // work out current indent
            self.goLineStart(edit_cursor);
            const this_line_start_pos = edit_cursor.head.pos;
            var this_indent: usize = 0;
            while (this_line_start_pos + this_indent < self_buffer.bytes.items.len and self_buffer.bytes.items[this_line_start_pos + this_indent] == ' ') {
                this_indent += 1;
            }
            const line_start_char = if (this_line_start_pos + this_indent < self_buffer.bytes.items.len) self_buffer.bytes.items[this_line_start_pos + this_indent] else 0;

            // work out prev line indent
            var prev_indent: usize = 0;
            if (edit_cursor.head.pos != 0) {
                self.goLeft(edit_cursor);
                const line_end_char = if (edit_cursor.head.pos > 0 and edit_cursor.head.pos - 1 < self_buffer.bytes.items.len) self_buffer.bytes.items[edit_cursor.head.pos - 1] else 0;

                self.goLineStart(edit_cursor);
                const prev_line_start_pos = edit_cursor.head.pos;
                while (prev_line_start_pos + prev_indent < self_buffer.bytes.items.len and self_buffer.bytes.items[prev_line_start_pos + prev_indent] == ' ') {
                    prev_indent += 1;
                }

                // add extra indent when opening a block
                // TODO this is kind of fragile
                switch (line_end_char) {
                    '(', '{', '[' => prev_indent += 4,
                    else => {},
                }
            } // else prev_indent=0 is fine

            // dedent when closing a block
            switch (line_start_char) {
                ')', '}', ']' => prev_indent = subSaturating(prev_indent, 4),
                else => {},
            }

            // adjust indent
            edit_cursor.head.pos = this_line_start_pos;
            if (this_indent > prev_indent) {
                self.delete(this_line_start_pos, this_line_start_pos + this_indent - prev_indent);
            }
            if (this_indent < prev_indent) {
                var spaces = self.app.frame_allocator.alloc(u8, prev_indent - this_indent) catch oom();
                std.mem.set(u8, spaces, ' ');
                // TODO this might delete the selection :S
                const old_marked = self.marked;
                self.marked = false;
                self.insert(edit_cursor, spaces);
                self.marked = old_marked;
            }

            // go to next line in selection
            self.goLineEnd(edit_cursor);
            self.goRight(edit_cursor);
        }
    }

    // this can happen eg after an automatic format
    pub fn correctInvalidCursors(self: *Editor) void {
        for (self.cursors.items) |*cursor| {
            for (&[_]*Point{ &cursor.head, &cursor.tail }) |point| {
                point.pos = min(point.pos, self.buffer().getBufferEnd());
            }
        }
    }

    pub fn tryFormat(self: *Editor) void {
        if (self.buffer().getFilename()) |filename| {
            if (std.mem.endsWith(u8, filename, ".zig")) {
                self.zigFormat();
            }
        }
    }

    pub fn zigFormat(self: *Editor) void {
        var self_buffer = self.buffer();
        var process = std.ChildProcess.init(
            &[_][]const u8{ "zig", "fmt", "--stdin" },
            self.app.frame_allocator,
        ) catch |err| panic("Error initing zig fmt: {}", .{err});
        process.stdin_behavior = .Pipe;
        process.stdout_behavior = .Pipe;
        process.stderr_behavior = .Pipe;
        process.spawn() catch |err| panic("Error spawning zig fmt: {}", .{err});
        process.stdin.?.outStream().writeAll(self_buffer.bytes.items) catch |err| panic("Error writing to zig fmt: {}", .{err});
        // TODO not sure if this is the correct way to signal input is complete
        process.stdin.?.close();
        process.stdin = null;
        const stderr = process.stderr.?.inStream().readAllAlloc(self.app.frame_allocator, 10 * 1024 * 1024) catch |err| panic("Error reading zig fmt stderr: {}", .{err});
        const stdout = process.stdout.?.inStream().readAllAlloc(self.app.frame_allocator, 10 * 1024 * 1024 * 1024) catch |err| panic("Error reading zig fmt stdout: {}", .{err});
        const result = process.wait() catch |err| panic("Error waiting for zig fmt: {}", .{err});
        assert(result == .Exited);
        if (result.Exited != 0) {
            warn("Error formatting zig buffer: {}", .{stderr});
        } else {
            self_buffer.replace(stdout);
            self.correctInvalidCursors();
        }
    }

    pub fn save(self: *Editor) void {
        var self_buffer = self.buffer();
        if (self_buffer.modified_since_last_save) {
            self.tryFormat();
            self_buffer.save();
        }
    }

    pub fn undo(self: *Editor) void {
        const pos_o = self.buffer().undo();
        if (pos_o) |pos| {
            self.collapseCursors();
            self.clearMark();
            var cursor = self.getMainCursor();
            self.goPos(cursor, pos);
            cursor.tail = cursor.head;
        }
    }

    pub fn redo(self: *Editor) void {
        const pos_o = self.buffer().redo();
        if (pos_o) |pos| {
            self.collapseCursors();
            self.clearMark();
            var cursor = self.getMainCursor();
            self.goPos(cursor, pos);
            cursor.tail = cursor.head;
        }
    }
};
