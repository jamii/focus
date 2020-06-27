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

pub const CursorData = struct {
    // 0 <= head_pos <= buffer.getBufferEnd()
    head_pos: usize,
    // what column the cursor 'wants' to be at
    // should only be updated by left/right movement
    head_col: usize,
    // 0 <= tail_pos <= buffer.getBufferEnd()
    tail_pos: ?usize,
    // allocated by view.allocator
    clipboard: []const u8,
};

pub const Cursor = struct {
    allocator: *Allocator,
    buffer: *Buffer,
    data: *CursorData,
    
    pub fn searchBackwards(self: Cursor, needle: []const u8) ?usize {
        return self.buffer.searchBackwards(self.data.head_pos, needle);
    }

    pub fn searchForwards(self: Cursor, needle: []const u8) ?usize {
        return self.buffer.searchForwards(self.data.head_pos, needle);
    }

    pub fn getLineStart(self: Cursor) usize {
        return self.searchBackwards("\n") orelse 0;
    }

    pub fn getLineEnd(self: Cursor) usize {
        return self.searchForwards("\n") orelse self.buffer.getBufferEnd();
    }

    pub fn updateCol(self: Cursor) void {
        self.data.head_col = self.data.head_pos - self.getLineStart();
    }

    pub fn goPos(self: Cursor, pos: usize) void {
        self.data.head_pos = pos;
        self.updateCol();
    }

    pub fn goCol(self: Cursor, col: usize) void {
        const line_start = self.getLineStart();
        self.data.head_col = min(col, self.getLineEnd() - line_start);
        self.data.head_pos = line_start + self.data.head_col;
    }

    pub fn goLine(self: Cursor, line: usize) void {
        self.data.head_pos = self.buffer.getLineStart(line);
        // leave head_col intact
    }

    pub fn goLineCol(self: Cursor, line: usize, col: usize) void {
        self.goLine(line);
        self.goCol(col);
    }

    pub fn goLeft(self: Cursor) void {
        self.data.head_pos -= @as(usize, if (self.data.head_pos == 0) 0 else 1);
        self.updateCol();
    }

    pub fn goRight(self: Cursor) void {
        self.data.head_pos += @as(usize, if (self.data.head_pos >= self.buffer.getBufferEnd()) 0 else 1);
        self.updateCol();
    }

    pub fn goDown(self: Cursor) void {
        if (self.searchForwards("\n")) |line_end| {
            const col = self.data.head_col;
            self.data.head_pos = line_end + 1;
            self.goCol(col);
            self.data.head_col = col;
        }
    }

    pub fn goUp(self: Cursor) void {
        if (self.searchBackwards("\n")) |line_start| {
            const col = self.data.head_col;
            self.data.head_pos = line_start - 1;
            self.goCol(col);
            self.data.head_col = col;
        }
    }

    pub fn goLineStart(self: Cursor) void {
        self.data.head_pos = self.getLineStart();
        self.data.head_col = 0;
    }

    pub fn goLineEnd(self: Cursor) void {
        self.data.head_pos = self.searchForwards("\n") orelse self.buffer.getBufferEnd();
        self.updateCol();
    }

    pub fn goPageStart(self: Cursor) void {
        self.goPos(0);
    }

    pub fn goPageEnd(self: Cursor) void {
        self.goPos(self.buffer.getBufferEnd());
    }

    pub fn deleteSelection(self: Cursor) void {
        if (self.data.tail_pos) |_| {
            const pos = self.getSelectionPos();
            self.buffer.delete(pos[0], pos[1]);
            self.data.head_pos = pos[0];
            self.data.head_col = pos[0];
        }
        self.clearMark();
    }

    pub fn deleteBackwards(self: Cursor) void {
        if (self.data.tail_pos) |_| {
            self.deleteSelection();
        } else if (self.data.head_pos > 0) {
            self.buffer.delete(self.data.head_pos-1, self.data.head_pos);
            self.goLeft();
        }
    }

    pub fn deleteForwards(self: Cursor) void {
        if (self.data.tail_pos) |_| {
            self.deleteSelection();
        } else if (self.data.head_pos < self.buffer.getBufferEnd()) {
            self.buffer.delete(self.data.head_pos, self.data.head_pos+1);
        }
    }

    pub fn insert(self: Cursor, chars: []const u8) ! void {
        self.deleteSelection();
        try self.buffer.insert(self.data.head_pos, chars);
        self.data.head_pos += chars.len;
        self.updateCol();
    }

    pub fn clearMark(self: Cursor) void {
        self.data.tail_pos = null;
    }

    pub fn setMarkPos(self: Cursor, pos: usize) void {
        self.data.tail_pos = pos;
    }

    pub fn setMark(self: Cursor) void {
        self.data.tail_pos = self.data.head_pos;
    }

    pub fn toggleMark(self: Cursor) void {
        if (self.data.tail_pos) |_| {
            self.clearMark();
        } else {
            self.setMark();
        }
    }

    pub fn getSelectionPos(self: Cursor) [2]usize {
        if (self.data.tail_pos) |tail_pos| {
            const selection_start_pos = min(self.data.head_pos, tail_pos);
            const selection_end_pos = max(self.data.head_pos, tail_pos);
            return [2]usize{selection_start_pos, selection_end_pos};
        } else {
            return [2]usize{self.data.head_pos, self.data.head_pos};
        }
    }

    pub fn getSelection(self: Cursor) ! []const u8 {
        const pos = self.getSelectionPos();
        return self.buffer.copy(self.allocator, pos[0], pos[1]);
    }

    pub fn copy(self: Cursor) ! void {
        self.allocator.free(self.data.clipboard);
        self.data.clipboard = try self.getSelection();
        self.clearMark();
    }

    pub fn cut(self: Cursor) ! void {
        self.allocator.free(self.data.clipboard);
        self.data.clipboard = try self.getSelection();
        self.deleteSelection();
    }

    pub fn paste(self: Cursor) ! void {
        try self.insert(self.data.clipboard);
    }
};

pub const View = struct {
    allocator: *Allocator,
    buffer: *Buffer,
    cursor_datas: ArrayList(CursorData),
    mouse_went_down_at: usize,

    pub fn init(allocator: *Allocator, buffer: *Buffer) ! View {
        var cursor_datas = ArrayList(CursorData).init(allocator);
        try cursor_datas.append(.{
                .head_pos=0,
                .head_col=0,
                .tail_pos=null,
                .clipboard="",
        });
        return View{
            .allocator = allocator,
            .buffer = buffer,
            .cursor_datas = cursor_datas,
            .mouse_went_down_at = 0,
        };
    }

    pub fn deinit(self: *View) void {
        for (self.cursors) |cursor_data| {
            self.allocator.free(cursor_data.clipboard);
        }
        self.cursors.deinit();
    }

    fn makeCursor(self: *View, data: *CursorData) Cursor {
        return Cursor{
            .allocator = self.allocator,
            .buffer = self.buffer,
            .data = data,
        };
    }

    pub fn frame(self: *View, ui: *UI, rect: UI.Rect) ! void {
        // TODO rctrl/ralt?
        const ctrl = 224;
        const alt = 226;

        // handle keys
        for (ui.key_went_down.items) |key| {
            if (ui.key_is_down[ctrl]) {
                switch (key) {
                    ' ' => for (self.cursor_datas.items) |*data| self.makeCursor(data).toggleMark(),
                    'c' => for (self.cursor_datas.items) |*data| try self.makeCursor(data).copy(),
                    'x' => for (self.cursor_datas.items) |*data| try self.makeCursor(data).cut(), // TODO
                    'v' => for (self.cursor_datas.items) |*data| try self.makeCursor(data).paste(), // TODO
                    'j' => for (self.cursor_datas.items) |*data| self.makeCursor(data).goLeft(),
                    'l' => for (self.cursor_datas.items) |*data| self.makeCursor(data).goRight(),
                    'k' => for (self.cursor_datas.items) |*data| self.makeCursor(data).goDown(),
                    'i' => for (self.cursor_datas.items) |*data| self.makeCursor(data).goUp(),
                    else => {},
                }
            } else if (ui.key_is_down[alt]) {
                switch (key) {
                    'j' => for (self.cursor_datas.items) |*data| self.makeCursor(data).goLineStart(),
                    'l' => for (self.cursor_datas.items) |*data| self.makeCursor(data).goLineEnd(),
                    'k' => for (self.cursor_datas.items) |*data| self.makeCursor(data).goPageEnd(),
                    'i' => for (self.cursor_datas.items) |*data| self.makeCursor(data).goPageStart(),
                    else => {},
                }
            } else {
                switch (key) {
                    8 => for (self.cursor_datas.items) |*data| self.makeCursor(data).deleteBackwards(), // TODO
                    13 => for (self.cursor_datas.items) |*data| try self.makeCursor(data).insert(&[1]u8{'\n'}), // TODO
                    79 => for (self.cursor_datas.items) |*data| self.makeCursor(data).goRight(),
                    80 => for (self.cursor_datas.items) |*data| self.makeCursor(data).goLeft(),
                    81 => for (self.cursor_datas.items) |*data| self.makeCursor(data).goDown(),
                    82 => for (self.cursor_datas.items) |*data| self.makeCursor(data).goUp(),
                    127 => for (self.cursor_datas.items) |*data| self.makeCursor(data).deleteForwards(), // TODO
                    else => if (key >= 32 and key <= 126) {
                        for (self.cursor_datas.items) |*data| try self.makeCursor(data).insert(&[1]u8{key}); // TODO
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
                    self.cursor_datas.shrink(0);
                }
                try self.cursor_datas.append(.{
                    .head_pos=0,
                    .head_col=0,
                    .tail_pos=null,
                    .clipboard="",
                });
            }
            const cursor = self.makeCursor(&self.cursor_datas.items[self.cursor_datas.items.len - 1]);
            cursor.goLineCol(line, col);
            if (ui.mouse_went_down[0]) {
                cursor.clearMark();
                self.mouse_went_down_at = cursor.data.head_pos;
            } else if (cursor.data.head_pos != self.mouse_went_down_at) {
                cursor.setMarkPos(self.mouse_went_down_at);
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

            for (self.cursor_datas.items) |*data| {
                const cursor = self.makeCursor(data);

                // draw cursor
                if (cursor.data.head_pos >= line_start_pos and cursor.data.head_pos <= line_end_pos) {
                    const x = rect.x + ((cursor.data.head_pos - line_start_pos) * atlas.max_char_width);
                    try ui.queueRect(.{.x = @intCast(u16, x), .y = y, .w=1, .h=atlas.text_height}, text_color);
                }

                // draw selection
                const selection_start_pos = min(cursor.data.head_pos, cursor.data.tail_pos orelse cursor.data.head_pos);
                const selection_end_pos = max(cursor.data.head_pos, cursor.data.tail_pos orelse cursor.data.head_pos);
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
