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

    pub fn bufferEnd(self: *Buffer) usize {
        return self.text.items.len;
    }

    pub fn lineStart(self: *Buffer, line: usize) usize {
        var pos: usize = 0;
        var lines_remaining = line;
        while (lines_remaining > 0) : (lines_remaining -= 1) {
            pos = if (self.searchForwards(pos, "\n")) |next_pos| next_pos + 1 else self.text.items.len;
        }
        return pos;
    }

    pub fn lineCol(self: *Buffer, pos: usize) [2]usize {
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
    // 0 <= head_pos <= buffer.bufferEnd()
    head_pos: usize,
    // what column the cursor 'wants' to be at
    // should only be updated by left/right movement
    head_col: usize,
    // 0 <= tail_pos <= buffer.bufferEnd()
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

    pub fn lineStart(self: Cursor) usize {
        return self.searchBackwards("\n") orelse 0;
    }

    pub fn lineEnd(self: Cursor) usize {
        return self.searchForwards("\n") orelse self.buffer.bufferEnd();
    }

    pub fn updateCol(self: Cursor) void {
        self.data.head_col = self.data.head_pos - self.lineStart();
    }

    pub fn goPos(self: Cursor, pos: usize) void {
        self.data.head_pos = pos;
        self.updateCol();
    }

    pub fn goCol(self: Cursor, col: usize) void {
        const line_start = self.lineStart();
        self.data.head_col = min(col, self.lineEnd() - line_start);
        self.data.head_pos = line_start + self.data.head_col;
    }

    pub fn goLine(self: Cursor, line: usize) void {
        self.data.head_pos = self.buffer.lineStart(line);
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
        self.data.head_pos += @as(usize, if (self.data.head_pos >= self.buffer.bufferEnd()) 0 else 1);
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
        self.data.head_pos = self.lineStart();
        self.data.head_col = 0;
    }

    pub fn goLineEnd(self: Cursor) void {
        self.data.head_pos = self.searchForwards("\n") orelse self.buffer.bufferEnd();
        self.updateCol();
    }

    pub fn goPageStart(self: Cursor) void {
        self.goPos(0);
    }

    pub fn goPageEnd(self: Cursor) void {
        self.goPos(self.buffer.bufferEnd());
    }

    pub fn deleteSelection(self: Cursor) void {
        if (self.selectionPos()) |pos| {
            self.buffer.delete(pos[0], pos[1]);
            self.data.head_pos = pos[0];
            self.data.head_col = pos[0];
        }
        self.clearMark();
    }

    pub fn deleteBackwards(self: Cursor) void {
        if (self.selectionPos()) |_| {
            self.deleteSelection();
        } else if (self.data.head_pos > 0) {
            self.buffer.delete(self.data.head_pos-1, self.data.head_pos);
            self.goLeft();
        }
    }

    pub fn deleteForwards(self: Cursor) void {
        if (self.selectionPos()) |_| {
            self.deleteSelection();
        } else if (self.data.head_pos < self.buffer.bufferEnd()) {
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

    pub fn selectionPos(self: Cursor) ?[2]usize {
        if (self.data.tail_pos) |tail_pos| {
            const selection_start_pos = min(self.data.head_pos, tail_pos);
            const selection_end_pos = max(self.data.head_pos, tail_pos);
            return [2]usize{selection_start_pos, selection_end_pos};
        } else {
            return null;
        }
    }

    pub fn selection(self: Cursor) ! []const u8 {
        if (self.selectionPos()) |pos| {
            return self.buffer.copy(self.allocator, pos[0], pos[1]);
        } else {
            return "";
        }
    }

    pub fn copy(self: Cursor) ! void {
        self.allocator.free(self.data.clipboard);
        self.data.clipboard = try self.selection();
        self.clearMark();
    }

    pub fn cut(self: Cursor) ! void {
        self.allocator.free(self.data.clipboard);
        self.data.clipboard = try self.selection();
        self.deleteSelection();
    }

    pub fn paste(self: Cursor) ! void {
        try self.insert(self.data.clipboard);
    }
};

pub const View = struct {
    allocator: *Allocator,
    buffer: *Buffer,
    cursor_data: CursorData,
    mouse_went_down_at: usize,

    pub fn init(allocator: *Allocator, buffer: *Buffer) View {
        return View{
            .allocator = allocator,
            .buffer = buffer,
            .cursor_data = .{
                .head_pos=0,
                .head_col=0,
                .tail_pos=null,
                .clipboard="",
            },
            .mouse_went_down_at = 0,
        };
    }

    pub fn deinit(self: *View) void {
        self.allocator.free(self.cursor_data.clipboard);
    }

    pub fn frame(self: *View, ui: *UI, rect: UI.Rect) ! void {
        const cursor = Cursor{
            .allocator = self.allocator,
            .buffer = self.buffer,
            .data = &self.cursor_data
        };
        
        for (ui.key_went_down.items) |key| {
            const ctrl = 224;
            const alt = 226;
            if (ui.key_is_down[ctrl]) {
                switch (key) {
                    ' ' => cursor.setMark(),
                    'c' => try cursor.copy(),
                    'x' => try cursor.cut(),
                    'v' => try cursor.paste(),
                    'j' => cursor.goLeft(),
                    'l' => cursor.goRight(),
                    'k' => cursor.goDown(),
                    'i' => cursor.goUp(),
                    else => {},
                }
            } else if (ui.key_is_down[alt]) {
                switch (key) {
                    ' ' => cursor.clearMark(),
                    'j' => cursor.goLineStart(),
                    'l' => cursor.goLineEnd(),
                    'k' => cursor.goPageEnd(),
                    'i' => cursor.goPageStart(),
                    else => {},
                }
            } else {
                switch (key) {
                    8 => cursor.deleteBackwards(),
                    13 => try cursor.insert(&[1]u8{'\n'}),
                    79 => cursor.goRight(),
                    80 => cursor.goLeft(),
                    81 => cursor.goDown(),
                    82 => cursor.goUp(),
                    127 => cursor.deleteForwards(),
                    else => if (key >= 32 and key <= 126) {
                        try cursor.insert(&[1]u8{key});
                    }
                }
            }
        }

        if (ui.mouse_is_down[0]) {
            const line = @divTrunc(ui.mouse_pos.y - rect.y, atlas.text_height);
            const col = @divTrunc(ui.mouse_pos.x - rect.x, atlas.max_char_width);
            cursor.goLineCol(line, col);
            if (ui.mouse_went_down[0]) {
                cursor.clearMark();
                self.mouse_went_down_at = cursor.data.head_pos;
            } else if (cursor.data.head_pos != self.mouse_went_down_at) {
                cursor.setMarkPos(self.mouse_went_down_at);
            }   
        }

        const text_color = UI.Color{ .r = 255, .g = 255, .b = 255, .a = 255 };
        const highlight_color = UI.Color{ .r = 255, .g = 255, .b = 255, .a = 100 };
        
        var lines = std.mem.split(self.buffer.text.items, "\n");
        var line_ix: u16 = 0;
        var line_start_pos: usize = 0;
        const selection_start_pos = min(cursor.data.head_pos, cursor.data.tail_pos orelse cursor.data.head_pos);
        const selection_end_pos = max(cursor.data.head_pos, cursor.data.tail_pos orelse cursor.data.head_pos);
        while (lines.next()) |line| : (line_ix += 1) {
            if ((line_ix * atlas.text_height) > rect.h) break;
            
            const y = rect.y + (line_ix * atlas.text_height);
            const line_end_pos = line_start_pos + line.len;

            // draw cursor
            if (cursor.data.head_pos >= line_start_pos and cursor.data.head_pos <= line_end_pos) {
                const x = rect.x + ((cursor.data.head_pos - line_start_pos) * atlas.max_char_width);
                try ui.queueRect(.{.x = @intCast(u16, x), .y = y, .w=1, .h=atlas.text_height}, text_color);
            }

            // draw selection
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
            
            // draw text
            try ui.queueText(.{.x = rect.x, .y = y}, text_color, line);
            
            line_start_pos = line_end_pos + 1; // + 1 for '\n'
        }
    }
};
