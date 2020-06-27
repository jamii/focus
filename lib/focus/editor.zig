const focus = @import("../focus.zig");
usingnamespace focus.common;
const atlas = focus.atlas;
const UI = focus.UI;

pub const Buffer = struct {
    allocator: *Allocator,
    text: ArrayList(u8),

    pub fn init(allocator: *Allocator) Buffer {
        return Buffer{
            .allocator = allocator,
            .text = ArrayList(u8).init(allocator),
        };
    }

    pub fn deinit(self: *Buffer) void {
        self.text.deinit();
    }

    pub fn getBufferEnd(self: *Buffer) usize {
        return self.text.items.len;
    }

    pub fn getLineStart(self: *Buffer, line: usize) usize {
        var pos: usize = 0;
        var lines_remaining = line;
        while (lines_remaining > 0) : (lines_remaining -= 1) {
            pos = if (self.searchForwards(pos, "\n")) |next_pos| next_pos + 1 else self.text.items.len;
        }
        return pos;
    }

    pub fn getLineColPos(self: *Buffer, pos: usize) [2]usize {
        var line: usize = 0;
        const col = pos - self.lineStart();
        var pos_remaining = pos;
        while (self.searchBackwards(pos_remaining, "\n")) |line_start| {
            pos_remaining = line_start - 1;
            line += 1;
        }
        return .{line, col};
    }

    pub fn searchBackwards(self: *Buffer, pos: usize, needle: []const u8) ?usize {
        const text = self.text.items[0..pos];
        return if (std.mem.lastIndexOf(u8, text, needle)) |result_pos| result_pos + needle.len else null;
    }

    pub fn searchForwards(self: *Buffer, pos: usize, needle: []const u8) ?usize {
        const text = self.text.items[pos..];
        return if (std.mem.indexOf(u8, text, needle)) |result_pos| result_pos + pos else null;
    }

    pub fn copy(self: *Buffer, allocator: *Allocator, start: usize, end: usize) ! []const u8 {
        assert(start <= end);
        assert(end <= self.text.items.len);
        return std.mem.dupe(allocator, u8, self.text.items[start..end]);
    }

    pub fn insert(self: *Buffer, pos: usize, chars: []const u8) ! void {
        try self.text.resize(self.text.items.len + chars.len);
        std.mem.copyBackwards(u8, self.text.items[pos+chars.len..], self.text.items[pos..self.text.items.len-chars.len]);
        std.mem.copy(u8, self.text.items[pos..], chars);
    }

    pub fn delete(self: *Buffer, start: usize, end: usize) void {
        assert(start <= end);
        assert(end <= self.text.items.len);
        std.mem.copy(u8, self.text.items[start..], self.text.items[end..]);
        self.text.shrink(self.text.items.len - (end - start));
    }

    pub fn and_cursor(self: *Buffer, cursor: *Cursor) BufferAndCursor {
        return BufferAndCursor{.buffer = self, .cursor = cursor};
    }
};

pub const Cursor = struct {
    // 0 <= head_pos <= buffer.getBufferEnd()
    head_pos: usize,
    // what column the cursor 'wants' to be at
    // should only be updated by left/right movement
    head_col: usize,
    // 0 <= tail_pos <= buffer.getBufferEnd()
    tail_pos: ?usize,
    // allocated by view.allocator
    clipboard: []const u8,

    pub fn getSelection(self: Cursor) [2]usize {
        if (self.tail_pos) |tail_pos| {
            const selection_start_pos = min(self.head_pos, tail_pos);
            const selection_end_pos = max(self.head_pos, tail_pos);
            return [2]usize{selection_start_pos, selection_end_pos};
        } else {
            return [2]usize{self.head_pos, self.head_pos};
        }
    }
};

pub const View = struct {
    allocator: *Allocator,
    buffer: *Buffer,
    cursors: ArrayList(Cursor),
    mouse_went_down_at: usize,

    pub fn init(allocator: *Allocator, buffer: *Buffer) ! View {
        var cursors = ArrayList(Cursor).init(allocator);
        try cursors.append(.{
                .head_pos=0,
                .head_col=0,
                .tail_pos=null,
                .clipboard="",
        });
        return View{
            .allocator = allocator,
            .buffer = buffer,
            .cursors = cursors,
            .mouse_went_down_at = 0,
        };
    }

    pub fn deinit(self: *View) void {
        for (self.cursors) |cursor| {
            self.allocator.free(cursor.clipboard);
        }
        self.cursors.deinit();
    }

    pub fn searchBackwards(self: *View, cursor: *Cursor, needle: []const u8) ?usize {
        return self.buffer.searchBackwards(cursor.head_pos, needle);
    }

    pub fn searchForwards(self: *View, cursor: *Cursor, needle: []const u8) ?usize {
        return self.buffer.searchForwards(cursor.head_pos, needle);
    }

    pub fn getLineStart(self: *View, cursor: *Cursor) usize {
        return self.searchBackwards(cursor, "\n") orelse 0;
    }

    pub fn getLineEnd(self: *View, cursor: *Cursor) usize {
        return self.searchForwards(cursor, "\n") orelse self.buffer.getBufferEnd();
    }

    pub fn updateCol(self: *View, cursor: *Cursor) void {
        cursor.head_col = cursor.head_pos - self.getLineStart(cursor);
    }

    pub fn goPos(self: *View, cursor: *Cursor, pos: usize) void {
        cursor.head_pos = pos;
    }

    pub fn goCol(self: *View, cursor: *Cursor, col: usize) void {
        const line_start = self.getLineStart(cursor);
        cursor.head_col = min(col, self.getLineEnd(cursor) - line_start);
        cursor.head_pos = line_start + cursor.head_col;
    }

    pub fn goLine(self: *View, cursor: *Cursor, line: usize) void {
        cursor.head_pos = self.buffer.getLineStart(line);
        // leave head_col intact
    }

    pub fn goLineCol(self: *View, cursor: *Cursor, line: usize, col: usize) void {
        self.goLine(cursor, line);
        self.goCol(cursor, col);
    }

    pub fn goLeft(self: *View, cursor: *Cursor) void {
        cursor.head_pos -= @as(usize, if (cursor.head_pos == 0) 0 else 1);
        self.updateCol(cursor);
    }

    pub fn goRight(self: *View, cursor: *Cursor) void {
        cursor.head_pos += @as(usize, if (cursor.head_pos >= self.buffer.getBufferEnd()) 0 else 1);
        self.updateCol(cursor);
    }

    pub fn goDown(self: *View, cursor: *Cursor) void {
        if (self.searchForwards(cursor, "\n")) |line_end| {
            const col = cursor.head_col;
            cursor.head_pos = line_end + 1;
            self.goCol(cursor, col);
            cursor.head_col = col;
        }
    }

    pub fn goUp(self: *View, cursor: *Cursor) void {
        if (self.searchBackwards(cursor, "\n")) |line_start| {
            const col = cursor.head_col;
            cursor.head_pos = line_start - 1;
            self.goCol(cursor, col);
            cursor.head_col = col;
        }
    }

    pub fn goLineStart(self: *View, cursor: *Cursor) void {
        cursor.head_pos = self.getLineStart(cursor);
        cursor.head_col = 0;
    }

    pub fn goLineEnd(self: *View, cursor: *Cursor) void {
        cursor.head_pos = self.searchForwards(cursor, "\n") orelse self.buffer.getBufferEnd();
        self.updateCol(cursor);
    }

    pub fn goPageStart(self: *View, cursor: *Cursor) void {
        self.goPos(cursor, 0);
    }

    pub fn goPageEnd(self: *View, cursor: *Cursor) void {
        self.goPos(cursor, self.buffer.getBufferEnd());
    }

    pub fn deleteSelection(self: *View, cursor: *Cursor) void {
        if (cursor.tail_pos) |_| {
            const pos = cursor.getSelection();
            self.buffer.delete(pos[0], pos[1]);
            cursor.head_pos = pos[0];
            cursor.head_col = pos[0];
        }
        self.clearMark(cursor);
    }

    pub fn deleteBackwards(self: *View, cursor: *Cursor) void {
        if (cursor.tail_pos) |_| {
            self.deleteSelection(cursor);
        } else if (cursor.head_pos > 0) {
            self.buffer.delete(cursor.head_pos-1, cursor.head_pos);
            self.goLeft(cursor);
        }
    }

    pub fn deleteForwards(self: *View, cursor: *Cursor) void {
        if (cursor.tail_pos) |_| {
            self.deleteSelection(cursor);
        } else if (cursor.head_pos < self.buffer.getBufferEnd()) {
            self.buffer.delete(cursor.head_pos, cursor.head_pos+1);
        }
    }

    pub fn insert(self: *View, cursor: *Cursor, chars: []const u8) ! void {
        self.deleteSelection(cursor);
        try self.buffer.insert(cursor.head_pos, chars);
        cursor.head_pos += chars.len;
        self.updateCol(cursor);
    }

    pub fn clearMark(self: *View, cursor: *Cursor) void {
        cursor.tail_pos = null;
    }

    pub fn setMarkPos(self: *View, cursor: *Cursor, pos: usize) void {
        cursor.tail_pos = pos;
    }

    pub fn setMark(self: *View, cursor: *Cursor) void {
        cursor.tail_pos = cursor.head_pos;
    }

    pub fn toggleMark(self: *View, cursor: *Cursor) void {
        if (cursor.tail_pos) |_| {
            self.clearMark(cursor);
        } else {
            self.setMark(cursor);
        }
    }

    pub fn getSelection(self: *View, cursor: *Cursor) ! []const u8 {
        const pos = cursor.getSelection();
        return self.buffer.copy(self.allocator, pos[0], pos[1]);
    }

    pub fn copy(self: *View, cursor: *Cursor) ! void {
        self.allocator.free(cursor.clipboard);
        cursor.clipboard = try self.getSelection(cursor);
        self.clearMark(cursor);
    }

    pub fn cut(self: *View, cursor: *Cursor) ! void {
        self.allocator.free(cursor.clipboard);
        cursor.clipboard = try self.getSelection(cursor);
        self.deleteSelection(cursor);
    }

    pub fn paste(self: *View, cursor: *Cursor) ! void {
        try self.insert(cursor, cursor.clipboard);
    }

    pub fn frame(self: *View, ui: *UI, rect: UI.Rect) ! void {
        // TODO rctrl/ralt?
        const ctrl = 224;
        const alt = 226;

        // handle keys
        for (ui.key_went_down.items) |key| {
            if (ui.key_is_down[ctrl]) {
                switch (key) {
                    ' ' => for (self.cursors.items) |*cursor| self.toggleMark(cursor),
                    'c' => for (self.cursors.items) |*cursor| try self.copy(cursor),
                    'x' => for (self.cursors.items) |*cursor| try self.cut(cursor), // TODO
                    'v' => for (self.cursors.items) |*cursor| try self.paste(cursor), // TODO
                    'j' => for (self.cursors.items) |*cursor| self.goLeft(cursor),
                    'l' => for (self.cursors.items) |*cursor| self.goRight(cursor),
                    'k' => for (self.cursors.items) |*cursor| self.goDown(cursor),
                    'i' => for (self.cursors.items) |*cursor| self.goUp(cursor),
                    else => {},
                }
            } else if (ui.key_is_down[alt]) {
                switch (key) {
                    'j' => for (self.cursors.items) |*cursor| self.goLineStart(cursor),
                    'l' => for (self.cursors.items) |*cursor| self.goLineEnd(cursor),
                    'k' => for (self.cursors.items) |*cursor| self.goPageEnd(cursor),
                    'i' => for (self.cursors.items) |*cursor| self.goPageStart(cursor),
                    else => {},
                }
            } else {
                switch (key) {
                    8 => for (self.cursors.items) |*cursor| self.deleteBackwards(cursor), // TODO
                    13 => for (self.cursors.items) |*cursor| try self.insert(cursor, &[1]u8{'\n'}), // TODO
                    79 => for (self.cursors.items) |*cursor| self.goRight(cursor),
                    80 => for (self.cursors.items) |*cursor| self.goLeft(cursor),
                    81 => for (self.cursors.items) |*cursor| self.goDown(cursor),
                    82 => for (self.cursors.items) |*cursor| self.goUp(cursor),
                    127 => for (self.cursors.items) |*cursor| self.deleteForwards(cursor), // TODO
                    else => if (key >= 32 and key <= 126) {
                        for (self.cursors.items) |*cursor| try self.insert(cursor, &[1]u8{key}); // TODO
                    }
                }
            }
        }

        // handle mouse
        if (ui.mouse_is_down[0]) {
            const line = @divTrunc(ui.mouse_pos.y - rect.y, atlas.text_height);
            const col = @divTrunc(ui.mouse_pos.x - rect.x, atlas.max_char_width);
            if (ui.mouse_went_down[0]) {
                if (!ui.key_is_down[ctrl]) {
                    self.cursors.shrink(0);
                }
                try self.cursors.append(.{
                    .head_pos=0,
                    .head_col=0,
                    .tail_pos=null,
                    .clipboard="",
                });
            }
            var cursor = &self.cursors.items[self.cursors.items.len - 1];
            self.goLineCol(cursor, line, col);
            if (ui.mouse_went_down[0]) {
                self.clearMark(cursor);
                self.mouse_went_down_at = cursor.head_pos;
            } else if (cursor.head_pos != self.mouse_went_down_at) {
                self.setMarkPos(cursor, self.mouse_went_down_at);
            }   
        }

        // draw
        const text_color = UI.Color{ .r = 255, .g = 255, .b = 255, .a = 255 };
        const highlight_color = UI.Color{ .r = 255, .g = 255, .b = 255, .a = 100 };
        var lines = std.mem.split(self.buffer.text.items, "\n");
        var line_ix: u16 = 0;
        var line_start_pos: usize = 0;
        while (lines.next()) |line| : (line_ix += 1) {
            if ((line_ix * atlas.text_height) > rect.h) break;
            
            const y = rect.y + (line_ix * atlas.text_height);
            const line_end_pos = line_start_pos + line.len;

            for (self.cursors.items) |*cursor| {
                // draw cursor
                if (cursor.head_pos >= line_start_pos and cursor.head_pos <= line_end_pos) {
                    const x = rect.x + ((cursor.head_pos - line_start_pos) * atlas.max_char_width);
                    try ui.queueRect(.{.x = @intCast(u16, x), .y = y, .w=1, .h=atlas.text_height}, text_color);
                }

                // draw selection
                const selection_start_pos = min(cursor.head_pos, cursor.tail_pos orelse cursor.head_pos);
                const selection_end_pos = max(cursor.head_pos, cursor.tail_pos orelse cursor.head_pos);
                const highlight_start_pos = min(max(selection_start_pos, line_start_pos), line_end_pos);
                const highlight_end_pos = min(max(selection_end_pos, line_start_pos), line_end_pos);
                if ((highlight_start_pos < highlight_end_pos) or (selection_start_pos <= line_end_pos and selection_end_pos > line_end_pos)) {
                    const x = rect.x + ((highlight_start_pos - line_start_pos) * atlas.max_char_width);
                    const w = if (selection_end_pos > line_end_pos)
                        rect.x + rect.w - x
                        else
                        (highlight_end_pos - highlight_start_pos) * atlas.max_char_width;
                    try ui.queueRect(.{.x = @intCast(u16, x), .y = y, .w=@intCast(u16, w), .h=atlas.text_height}, highlight_color);
                }
            }
            
            // draw text
            try ui.queueText(.{.x = rect.x, .y = y}, text_color, line);
            
            line_start_pos = line_end_pos + 1; // + 1 for '\n'
        }
    }
};
