const focus = @import("../focus.zig");
usingnamespace focus.common;
const atlas = focus.atlas;
const UI = focus.UI;

const Cursor = struct {
    pos: usize,
    col: usize,
};

pub const Editor = struct {
    allocator: *Allocator,
    text: ArrayList(u8),
    cursor: Cursor,

    pub fn init(allocator: *Allocator, init_text: []const u8) ! Editor {
        var text = try ArrayList(u8).initCapacity(allocator, init_text.len);
        try text.appendSlice(init_text);
        return Editor{.allocator = allocator, .text = text, .cursor = .{.pos=0, .col=0}};
    }

    pub fn deinit(self: *Editor) void {
        self.text.deinit();
    }

    const white = UI.Color{ .r = 255, .g = 255, .b = 255, .a = 255 };

    fn searchBackwards(self: *Editor, needle: []const u8) ?usize {
        const text = self.text.items[0..self.cursor.pos];
        return if (std.mem.lastIndexOf(u8, text, needle)) |pos| pos + needle.len else null;
    }

    fn searchForwards(self: *Editor, needle: []const u8) ?usize {
        const text = self.text.items[self.cursor.pos..];
        return if (std.mem.indexOf(u8, text, needle)) |pos| pos + self.cursor.pos else null;
    }

    fn updateCol(self: *Editor) void {
        self.cursor.col = self.cursor.pos - (self.searchBackwards("\n") orelse 0);
    }

    fn goLeftPreserveCol(self: *Editor) void {
        self.cursor.pos -= @as(usize, if (self.cursor.pos == 0) 0 else 1);
    }

    fn goRightPreserveCol(self: *Editor) void {
        self.cursor.pos += @as(usize, if (self.cursor.pos >= self.text.items.len) 0 else 1);
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
        self.cursor.pos = self.searchBackwards("\n") orelse 0;
    }

    fn goLineEnd(self: *Editor) void {
        self.cursor.pos = self.searchForwards("\n") orelse self.text.items.len;
    }

    fn goDown(self: *Editor) void {
        if (self.searchForwards("\n")) |line_end| {
            self.cursor.pos = line_end;
            self.goRightPreserveCol();
            const line_len = (self.searchForwards("\n") orelse self.text.items.len) - self.cursor.pos;
            self.cursor.pos += min(self.cursor.col, line_len);
        }
    }

    fn goUp(self: *Editor) void {
        self.goLineStart();
        self.goLeftPreserveCol();
        self.goLineStart();
        const line_len = (self.searchForwards("\n") orelse self.text.items.len) - self.cursor.pos;
        self.cursor.pos += min(self.cursor.col, line_len);
    }

    fn deleteBackwards(self: *Editor) void {
        if (self.cursor.pos > 0) {
            std.mem.copy(u8, self.text.items[self.cursor.pos-1..], self.text.items[self.cursor.pos..]);
            _ = self.text.pop();
            self.goLeft();
        }
    }

    fn deleteForwards(self: *Editor) void {
        if (self.cursor.pos < self.text.items.len) {
            std.mem.copy(u8, self.text.items[self.cursor.pos..], self.text.items[self.cursor.pos+1..]);
            _ = self.text.pop();
        }
    }

    fn insertChar(self: *Editor, char: u8) ! void {
        try self.text.append(0);
        std.mem.copyBackwards(u8, self.text.items[self.cursor.pos+1..], self.text.items[self.cursor.pos..self.text.items.len-1]);
        self.text.items[self.cursor.pos] = char;
        self.goRight();
    }

    pub fn frame(self: *Editor, ui: *UI, rect: UI.Rect) ! void {
        if (ui.key_went_down) {
            const key = ui.key.?;
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
            dump(key);
        }


        var lines = std.mem.split(self.text.items, "\n");
        var line_ix: u16 = 0;
        var pos: usize = 0;
        while (lines.next()) |line| : (line_ix += 1) {
            if ((line_ix * atlas.text_height) > rect.h) break;
            const y = rect.y + (line_ix * atlas.text_height);
            try ui.queueText(.{.x = rect.x, .y = y}, white, line);
            const line_len = line.len + 1; // + 1 for "\n"
            if (self.cursor.pos >= pos and self.cursor.pos < pos + line_len) {
                const x = rect.x + ((self.cursor.pos - pos) * atlas.max_char_width);
                try ui.queueRect(.{.x = @intCast(u16, x), .y = y, .w=1, .h=atlas.text_height}, white);
            }
            pos += line_len;
        }
    }
};
