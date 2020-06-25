const focus = @import("../focus.zig");
usingnamespace focus.common;
const atlas = focus.atlas;
const UI = focus.UI;

const Cursor = struct {
    head_pos: usize,
    head_col: usize,
    tail_pos: usize,
};

pub const Editor = struct {
    allocator: *Allocator,
    text: ArrayList(u8),
    cursor: Cursor,

    pub fn init(allocator: *Allocator, init_text: []const u8) ! Editor {
        var text = try ArrayList(u8).initCapacity(allocator, init_text.len);
        try text.appendSlice(init_text);
        return Editor{.allocator = allocator, .text = text, .cursor = .{.head_pos=0, .head_col=0, .tail_pos=0}};
    }

    pub fn deinit(self: *Editor) void {
        self.text.deinit();
    }

    fn searchBackwards(self: *Editor, needle: []const u8) ?usize {
        const text = self.text.items[0..self.cursor.head_pos];
        return if (std.mem.lastIndexOf(u8, text, needle)) |pos| pos + needle.len else null;
    }

    fn searchForwards(self: *Editor, needle: []const u8) ?usize {
        const text = self.text.items[self.cursor.head_pos..];
        return if (std.mem.indexOf(u8, text, needle)) |pos| pos + self.cursor.head_pos else null;
    }

    fn updateCol(self: *Editor) void {
        self.cursor.head_col = self.cursor.head_pos - (self.searchBackwards("\n") orelse 0);
    }

    fn goLeftPreserveCol(self: *Editor) void {
        self.cursor.head_pos -= @as(usize, if (self.cursor.head_pos == 0) 0 else 1);
    }

    fn goRightPreserveCol(self: *Editor) void {
        self.cursor.head_pos += @as(usize, if (self.cursor.head_pos >= self.text.items.len) 0 else 1);
    }

    fn goLeft(self: *Editor) void {
        self.goLeftPreserveCol();
        self.updateCol();
    }

    fn goRight(self: *Editor) void {
        self.goRightPreserveCol();
        self.updateCol();
    }

    fn goLineStart(self: *Editor) void {
        self.cursor.head_pos = self.searchBackwards("\n") orelse 0;
    }

    fn goLineEnd(self: *Editor) void {
        self.cursor.head_pos = self.searchForwards("\n") orelse self.text.items.len;
    }

    fn goDown(self: *Editor) void {
        if (self.searchForwards("\n")) |line_end| {
            self.cursor.head_pos = line_end;
            self.goRightPreserveCol();
            self.goCol(self.cursor.head_col);
        }
    }

    fn goUp(self: *Editor) void {
        self.goLineStart();
        self.goLeftPreserveCol();
        self.goLineStart();
        self.goCol(self.cursor.head_col);
    }

    fn deleteBackwards(self: *Editor) void {
        if (self.cursor.head_pos > 0) {
            std.mem.copy(u8, self.text.items[self.cursor.head_pos-1..], self.text.items[self.cursor.head_pos..]);
            _ = self.text.pop();
            self.goLeft();
        }
    }

    fn deleteForwards(self: *Editor) void {
        if (self.cursor.head_pos < self.text.items.len) {
            std.mem.copy(u8, self.text.items[self.cursor.head_pos..], self.text.items[self.cursor.head_pos+1..]);
            _ = self.text.pop();
        }
    }

    fn insertChar(self: *Editor, char: u8) ! void {
        try self.text.append(0);
        std.mem.copyBackwards(u8, self.text.items[self.cursor.head_pos+1..], self.text.items[self.cursor.head_pos..self.text.items.len-1]);
        self.text.items[self.cursor.head_pos] = char;
        self.goRight();
    }

    fn goCol(self: *Editor, col: usize) void {
        const line_len = (self.searchForwards("\n") orelse self.text.items.len) - self.cursor.head_pos;
        self.cursor.head_pos += min(col, line_len);
    }

    fn goLine(self: *Editor, line: usize) void {
        self.cursor.head_pos = 0;
        var lines_remaining = line;
        while (lines_remaining > 0) : (lines_remaining -= 1) {
            self.goLineEnd();
            self.goRight();
        }
    }

    fn goLineCol(self: *Editor, line: usize, col: usize) void {
        self.goLine(line);
        self.goCol(col);
    }

    pub fn frame(self: *Editor, ui: *UI, rect: UI.Rect) ! void {
        for (ui.key_went_down.items) |key| {
            switch (key) {
                8 => self.deleteBackwards(),
                13 => try self.insertChar('\n'),
                79 => self.goRight(),
                80 => self.goLeft(),
                81 => self.goDown(),
                82 => self.goUp(),
                127 => self.deleteForwards(),
                else => if (key >= 32 and key <= 126) {
                    try self.insertChar(key);
                }
            }
        }

        if (ui.mouse_is_down[0]) {
            const line = @divTrunc(ui.mouse_pos.y - rect.y, atlas.text_height);
            const col = @divTrunc(ui.mouse_pos.x - rect.x, atlas.max_char_width);
            self.goLineCol(line, col);
        }

        const text_color = UI.Color{ .r = 255, .g = 255, .b = 255, .a = 255 };
        const highlight_color = UI.Color{ .r = 255, .g = 255, .b = 255, .a = 100 };
        
        var lines = std.mem.split(self.text.items, "\n");
        var line_ix: u16 = 0;
        var line_start_pos: usize = 0;
        const selection_start_pos = min(self.cursor.head_pos, self.cursor.tail_pos);
        const selection_end_pos = max(self.cursor.head_pos, self.cursor.tail_pos);
        while (lines.next()) |line| : (line_ix += 1) {
            if ((line_ix * atlas.text_height) > rect.h) break;
            
            const y = rect.y + (line_ix * atlas.text_height);
            const line_end_pos = line_start_pos + line.len;

            // draw cursor
            if (self.cursor.head_pos >= line_start_pos and self.cursor.head_pos <= line_end_pos) {
                const x = rect.x + ((self.cursor.head_pos - line_start_pos) * atlas.max_char_width);
                try ui.queueRect(.{.x = @intCast(u16, x), .y = y, .w=1, .h=atlas.text_height}, text_color);
            }

            // draw selection
            const highlight_start_pos = min(max(selection_start_pos, line_start_pos), line_end_pos);
            const highlight_end_pos = min(max(selection_end_pos, line_start_pos), line_end_pos);
            if (highlight_start_pos < highlight_end_pos) {
                const x = rect.x + ((highlight_start_pos - line_start_pos) * atlas.max_char_width);
                const w = (highlight_end_pos - highlight_start_pos) * atlas.max_char_width;
                try ui.queueRect(.{.x = @intCast(u16, x), .y = y, .w=@intCast(u16, w), .h=atlas.text_height}, highlight_color);
            }
            
            // draw text
            try ui.queueText(.{.x = rect.x, .y = y}, text_color, line);
            
            line_start_pos = line_end_pos + 1; // + 1 for '\n'
        }
    }
};
