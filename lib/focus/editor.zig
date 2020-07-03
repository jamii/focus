const focus = @import("../focus.zig");
usingnamespace focus.common;
const Buffer = focus.Buffer;
const Window = focus.Window;

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
    // allocated by editor.allocator
    clipboard: []const u8,
};

pub const Dragging = enum {
    NotDragging,
    Dragging,
    CtrlDragging,
};

pub const Editor = struct {
    allocator: *Allocator,
    buffer: *Buffer,
    // cursors.len > 0
    cursors: ArrayList(Cursor),
    marked: bool,
    dragging: Dragging,
    // which pixel of the buffer is at the top of the viewport
    top_pixel: isize,

    const scroll_amount = 16;

    pub fn init(allocator: *Allocator, buffer: *Buffer) ! Editor {
        var cursors = ArrayList(Cursor).init(allocator);
        try cursors.append(.{
            .head = .{.pos=0, .col=0},
            .tail = .{.pos=0, .col=0},
            .clipboard="",
        });
        return Editor{
            .allocator = allocator,
            .buffer = buffer,
            .cursors = cursors,
            .marked = false,
            .dragging = .NotDragging,
            .top_pixel = 0,
        };
    }

    pub fn deinit(self: *Editor) void {
        for (self.cursors.items) |cursor| {
            self.allocator.free(cursor.clipboard);
        }
        self.cursors.deinit();
    }

    pub fn frame(self: *Editor, window: *Window, rect: Rect) ! void {
        const main_cursor_pos = self.getMainCursor().head.pos;
        
        // handle events
        // if we get textinput, we'll also get the keydown first
        // if the keydown is mapped to a command, we'll do that and ignore the textinput
        // TODO this assumes that they always arrive in the same frame, which the sdl docs are not clear about
        var accept_textinput = false;
        for (window.events.items) |event| {
            switch (event.type) {
                c.SDL_KEYDOWN => {
                    const sym = event.key.keysym;
                    if (sym.mod & @intCast(u16, c.KMOD_CTRL) != 0) {
                        switch (sym.sym) {
                            ' ' => self.toggleMark(),
                            'c' => {
                                for (self.cursors.items) |*cursor| try self.copy(cursor);
                                self.clearMark();
                            },
                            'x' => {
                                for (self.cursors.items) |*cursor| try self.cut(cursor);
                                self.clearMark();
                            },
                            'v' => {
                                for (self.cursors.items) |*cursor| try self.paste(cursor);
                                self.clearMark();
                            },
                            'j' => for (self.cursors.items) |*cursor| self.goLeft(cursor),
                            'l' => for (self.cursors.items) |*cursor| self.goRight(cursor),
                            'k' => for (self.cursors.items) |*cursor| self.goDown(cursor),
                            'i' => for (self.cursors.items) |*cursor| self.goUp(cursor),
                            'q' => {
                                try self.collapseCursors();
                                self.clearMark();
                            },
                            'd' => {
                                try self.addNextMatch();
                            },
                            else => accept_textinput = true,
                        }
                    } else if (sym.mod & @intCast(u16, c.KMOD_ALT) != 0) {
                        switch (sym.sym) {
                            ' ' => for (self.cursors.items) |*cursor| self.swapHead(cursor),
                            'j' => for (self.cursors.items) |*cursor| self.goLineStart(cursor),
                            'l' => for (self.cursors.items) |*cursor| self.goLineEnd(cursor),
                            'k' => for (self.cursors.items) |*cursor| self.goPageEnd(cursor),
                            'i' => for (self.cursors.items) |*cursor| self.goPageStart(cursor),
                            else => accept_textinput = true,
                        }
                    } else {
                        switch (sym.sym) {
                            c.SDLK_BACKSPACE => {
                                for (self.cursors.items) |*cursor| self.deleteBackwards(cursor);
                                self.clearMark();
                            },
                            c.SDLK_RETURN => {
                                for (self.cursors.items) |*cursor| try self.insert(cursor, &[1]u8{'\n'});
                                self.clearMark();
                            },
                            c.SDLK_RIGHT => for (self.cursors.items) |*cursor| self.goRight(cursor),
                            c.SDLK_LEFT => for (self.cursors.items) |*cursor| self.goLeft(cursor),
                            c.SDLK_DOWN => for (self.cursors.items) |*cursor| self.goDown(cursor),
                            c.SDLK_UP => for (self.cursors.items) |*cursor| self.goUp(cursor),
                            c.SDLK_DELETE => {
                                for (self.cursors.items) |*cursor| self.deleteForwards(cursor);
                                self.clearMark();
                            },
                            else => accept_textinput = true,
                        }
                    }
                },
                c.SDL_TEXTINPUT => {
                    if (accept_textinput) {
                        const text = event.text.text[0..std.mem.indexOfScalar(u8, &event.text.text, 0).?];
                        for (self.cursors.items) |*cursor| try self.insert(cursor, text);
                        self.clearMark();
                    }
                },
                c.SDL_MOUSEBUTTONDOWN => {
                    const button = event.button;
                    if (button.button == c.SDL_BUTTON_LEFT) {
                        const line = @divTrunc(self.top_pixel + @intCast(Coord, button.y) - rect.y, window.atlas.char_height);
                        const col = @divTrunc(@intCast(Coord, button.x) - rect.x + @divTrunc(window.atlas.char_width, 2), window.atlas.char_width);
                        const pos = self.buffer.getPosForLineCol(@intCast(usize, max(line, 0)), @intCast(usize, max(col, 0)));
                        if (@enumToInt(c.SDL_GetModState()) & c.KMOD_CTRL != 0) {
                            self.dragging = .CtrlDragging;
                            var cursor = try self.newCursor();
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
            const line = @divTrunc(self.top_pixel + mouse_y - rect.y, window.atlas.char_height);
            const col = @divTrunc(mouse_x - rect.x + @divTrunc(window.atlas.char_width, 2), window.atlas.char_width);
            const pos = self.buffer.getPosForLineCol(@intCast(usize, max(line, 0)), @intCast(usize, max(col, 0)));
            if (self.dragging == .CtrlDragging) {
                var cursor = &self.cursors.items[self.cursors.items.len-1];
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
            if (mouse_y <= rect.y) self.top_pixel -= scroll_amount;
            if (mouse_y >= rect.y + rect.h) self.top_pixel += scroll_amount;
        }

        // if cursor moved, scroll it into editor
        if (self.getMainCursor().head.pos != main_cursor_pos) {
            const bottom_pixel = self.top_pixel + rect.h;
            const cursor_top_pixel = @intCast(isize, self.buffer.getLineColForPos(self.getMainCursor().head.pos)[0]) * @intCast(isize, window.atlas.char_height);
            const cursor_bottom_pixel = cursor_top_pixel + @intCast(isize, window.atlas.char_height);
            if (cursor_top_pixel > bottom_pixel - @intCast(isize, window.atlas.char_height))
                self.top_pixel = cursor_top_pixel - @intCast(isize, rect.h) + @intCast(isize, window.atlas.char_height);
            if (cursor_bottom_pixel <= self.top_pixel + @intCast(isize, window.atlas.char_height))
                self.top_pixel = cursor_top_pixel;
        }

        // calculate visible range
        // ensure we don't scroll off the top or bottom of the buffer
        const max_pixels = @intCast(isize, self.buffer.countLines()) * @intCast(isize, window.atlas.char_height);
        if (self.top_pixel < 0) self.top_pixel = 0;
        if (self.top_pixel > max_pixels) self.top_pixel = max_pixels;
        const num_visible_lines = @divTrunc(rect.h, window.atlas.char_height) + @rem(@rem(rect.h, window.atlas.char_height), 1); // round up
        const visible_start_line = @divTrunc(self.top_pixel, window.atlas.char_height); // round down
        const visible_end_line = visible_start_line + num_visible_lines;

        // draw background
        const background_color = Color{ .r = 0x2e, .g=0x34, .b=0x36, .a=255 };
        try window.queueRect(rect, background_color);

        // draw cursors, selections, text
        const text_color = Color{ .r = 0xee, .g = 0xee, .b = 0xec, .a = 255 };
        const multi_cursor_color = Color{ .r = 0x7a, .g = 0xa6, .b = 0xda, .a = 255 };
        var highlight_color = text_color; highlight_color.a = 100;
        var lines = std.mem.split(self.buffer.bytes.items, "\n");
        var line_ix: usize = 0;
        var line_start_pos: usize = 0;
        while (lines.next()) |line| : (line_ix += 1) {
            if (line_ix > visible_end_line) break;

            const line_end_pos = line_start_pos + line.len;
            
            if (line_ix >= visible_start_line) {
                const y = rect.y - @rem(self.top_pixel+1, window.atlas.char_height) + ((@intCast(Coord, line_ix) - visible_start_line) * window.atlas.char_height);

                for (self.cursors.items) |cursor| {
                    // draw cursor
                    if (cursor.head.pos >= line_start_pos and cursor.head.pos <= line_end_pos) {
                        const x = rect.x + (@intCast(Coord, (cursor.head.pos - line_start_pos)) * window.atlas.char_width);
                        const w = @divTrunc(window.atlas.char_width, 8);
                        try window.queueRect(
                            .{
                                .x = @intCast(Coord, x) - @divTrunc(w, 2),
                                .y = @intCast(Coord, y),
                                .w=w,
                                .h=window.atlas.char_height
                            },
                            if (self.cursors.items.len > 1) multi_cursor_color else text_color,
                        );
                    }

                    // draw selection
                    if (self.marked) {
                        const selection_start_pos = min(cursor.head.pos, cursor.tail.pos);
                        const selection_end_pos = max(cursor.head.pos, cursor.tail.pos);
                        const highlight_start_pos = min(max(selection_start_pos, line_start_pos), line_end_pos);
                        const highlight_end_pos = min(max(selection_end_pos, line_start_pos), line_end_pos);
                        if ((highlight_start_pos < highlight_end_pos)
                                or (selection_start_pos <= line_end_pos
                                        and selection_end_pos > line_end_pos)) {
                            const x = rect.x + (@intCast(Coord, (highlight_start_pos - line_start_pos)) * window.atlas.char_width);
                            const w = if (selection_end_pos > line_end_pos)
                                rect.x + rect.w - x
                                else
                                @intCast(Coord, (highlight_end_pos - highlight_start_pos)) * window.atlas.char_width;
                            try window.queueRect(
                                .{
                                    .x = @intCast(Coord, x),
                                    .y = @intCast(Coord, y),
                                    .w = @intCast(Coord, w),
                                    .h = window.atlas.char_height,
                                },
                                highlight_color
                            );
                        }
                    }
                }
                
                // draw text
                // TODO need to ensure this text lives long enough - buffer might get changed in another window
                try window.queueText(.{.x = rect.x, .y = @intCast(Coord, y)}, text_color, line);
            }
            
            line_start_pos = line_end_pos + 1; // + 1 for '\n'
        }

        // draw scrollbar
        {
            const ratio = @intToFloat(f64, self.top_pixel) / @intToFloat(f64, max_pixels);
            const y = rect.y + min(@floatToInt(Coord, @intToFloat(f64, rect.h) * ratio), rect.h - window.atlas.char_height);
            const x = rect.x + rect.w - window.atlas.char_width;
            try window.queueText(.{.x = x, .y = y}, highlight_color, "<");
        }
    }

    pub fn updateCol(self: *Editor, point: *Point) void {
        point.col = point.pos - self.buffer.getLineStart(point.*.pos);
    }

    pub fn updatePos(self: *Editor, point: *Point, pos: usize) void {
        point.pos = pos;
        self.updateCol(point);
    }

    pub fn goPos(self: *Editor, cursor: *Cursor, pos: usize) void {
        self.updatePos(&cursor.head, pos);
    }

    pub fn goCol(self: *Editor, cursor: *Cursor, col: usize) void {
        const line_start = self.buffer.getLineStart(cursor.head.pos);
        cursor.head.col = min(col, self.buffer.getLineEnd(cursor.head.pos) - line_start);
        cursor.head.pos = line_start + cursor.head.col;
    }

    pub fn goLine(self: *Editor, cursor: *Cursor, line: usize) void {
        cursor.head.pos = self.buffer.getPosForLine(line);
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
        cursor.head.pos += @as(usize, if (cursor.head.pos >= self.buffer.getBufferEnd()) 0 else 1);
        self.updateCol(&cursor.head);
    }

    pub fn goDown(self: *Editor, cursor: *Cursor) void {
        if (self.buffer.searchForwards(cursor.head.pos, "\n")) |line_end| {
            const col = cursor.head.col;
            cursor.head.pos = line_end + 1;
            self.goCol(cursor, col);
            cursor.head.col = col;
        }
    }

    pub fn goUp(self: *Editor, cursor: *Cursor) void {
        if (self.buffer.searchBackwards(cursor.head.pos, "\n")) |line_start| {
            const col = cursor.head.col;
            cursor.head.pos = line_start - 1;
            self.goCol(cursor, col);
            cursor.head.col = col;
        }
    }

    pub fn goLineStart(self: *Editor, cursor: *Cursor) void {
        cursor.head.pos = self.buffer.getLineStart(cursor.head.pos);
        cursor.head.col = 0;
    }

    pub fn goLineEnd(self: *Editor, cursor: *Cursor) void {
        cursor.head.pos = self.buffer.getLineEnd(cursor.head.pos);
        self.updateCol(&cursor.head);
    }

    pub fn goPageStart(self: *Editor, cursor: *Cursor) void {
        self.goPos(cursor, 0);
    }

    pub fn goPageEnd(self: *Editor, cursor: *Cursor) void {
        self.goPos(cursor, self.buffer.getBufferEnd());
    }

    pub fn insert(self: *Editor, cursor: *Cursor, bytes: []const u8) ! void {
        self.deleteSelection(cursor);
        try self.buffer.insert(cursor.head.pos, bytes);
        const insert_at = cursor.head.pos;
        for (self.cursors.items) |*other_cursor| {
            for (&[2]*Point{&other_cursor.head, &other_cursor.tail}) |point| {
                // ptr compare is because we want paste to leave each cursor after its own insert
                if (point.pos > insert_at or (point.pos == insert_at and @ptrToInt(other_cursor) >= @ptrToInt(cursor))) {
                    point.pos += bytes.len;
                    self.updateCol(point);
                }
            }
        }
    }

    pub fn delete(self: *Editor, start: usize, end: usize) void {
        assert(start <= end);
        self.buffer.delete(start, end);
        for (self.cursors.items) |*other_cursor| {
            for (&[2]*Point{&other_cursor.head, &other_cursor.tail}) |point| {
                if (point.pos >= start and point.pos <= end) point.pos = start;
                if (point.pos > end) point.pos -= (end - start);
                self.updateCol(point);
            }
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
            self.delete(cursor.head.pos-1, cursor.head.pos);
        }
    }

    pub fn deleteForwards(self: *Editor, cursor: *Cursor) void {
        if (self.marked) {
            self.deleteSelection(cursor);
        } else if (cursor.head.pos < self.buffer.getBufferEnd()) {
            self.delete(cursor.head.pos, cursor.head.pos+1);
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
            return [2]usize{selection_start_pos, selection_end_pos};
        } else {
            return [2]usize{cursor.head.pos, cursor.head.pos};
        }
    }

    pub fn dupeSelection(self: *Editor, cursor: *Cursor) ! []const u8 {
        const range = self.getSelectionRange(cursor);
        return self.buffer.dupe(self.allocator, range[0], range[1]);
    }

    pub fn copy(self: *Editor, cursor: *Cursor) ! void {
        self.allocator.free(cursor.clipboard);
        cursor.clipboard = try self.dupeSelection(cursor);
    }

    pub fn cut(self: *Editor, cursor: *Cursor) ! void {
        self.allocator.free(cursor.clipboard);
        cursor.clipboard = try self.dupeSelection(cursor);
        self.deleteSelection(cursor);
    }

    pub fn paste(self: *Editor, cursor: *Cursor) ! void {
        try self.insert(cursor, cursor.clipboard);
    }

    pub fn newCursor(self: *Editor) ! *Cursor {
        try self.cursors.append(.{
            .head = .{.pos=0, .col=0},
            .tail = .{.pos=0, .col=0},
            .clipboard="",
        });
        return &self.cursors.items[self.cursors.items.len-1];
    }

    pub fn collapseCursors(self: *Editor) ! void {
        var size: usize = 0;
        for (self.cursors.items) |cursor| {
            size += cursor.clipboard.len;
        }
        var clipboard = try ArrayList(u8).initCapacity(self.allocator, size);
        for (self.cursors.items) |cursor| {
            clipboard.appendSlice(cursor.clipboard) catch unreachable;
            self.allocator.free(cursor.clipboard);
        }
        self.cursors.shrink(1);
        self.cursors.items[0].clipboard = clipboard.toOwnedSlice();
    }

    pub fn getMainCursor(self: *Editor) *Cursor {
        return &self.cursors.items[self.cursors.items.len-1];
    }

    pub fn addNextMatch(self: *Editor) ! void {
        const main_cursor = self.getMainCursor();
        const selection = try self.dupeSelection(main_cursor);
        defer self.allocator.free(selection);
        if (self.buffer.searchForwards(max(main_cursor.head.pos, main_cursor.tail.pos), selection)) |pos| {
            var cursor = Cursor{
                .head = .{.pos = pos + selection.len, .col = 0},
                .tail = .{.pos = pos, .col = 0},
                .clipboard = "",
            };
            self.updateCol(&cursor.head);
            self.updateCol(&cursor.tail);
            try self.cursors.append(cursor);
        }
    }
};
