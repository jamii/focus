const focus = @import("../focus.zig");
usingnamespace focus.common;
const atlas = focus.atlas;
const UI = focus.UI;

const Cursor = struct {
    head_pos: usize,
    head_col: usize,
    tail_pos: ?usize,
};

pub const Editor = struct {
    allocator: *Allocator,
    text: ArrayList(u8),
    cursor: Cursor,
    clipboard: ?[]const u8,
    mouse_went_down_at: Cursor,

    pub fn init(allocator: *Allocator, init_text: []const u8) ! Editor {
        var text = try ArrayList(u8).initCapacity(allocator, init_text.len);
        try text.appendSlice(init_text);
        return Editor{
            .allocator = allocator,
            .text = text,
            .cursor = .{.head_pos=0, .head_col=0, .tail_pos=null},
            .clipboard=null,
            .mouse_went_down_at = .{.head_pos=0, .head_col=0, .tail_pos=null},
        };
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

    fn lineStart(self: *Editor) usize {
        return self.searchBackwards("\n") orelse 0;
    }

    fn lineEnd(self: *Editor) usize {
        return self.searchForwards("\n") orelse self.text.items.len;
    }

    fn updateCol(self: *Editor) void {
        self.cursor.head_col = self.cursor.head_pos - self.lineStart();
    }

    fn goPos(self: *Editor, pos: usize) void {
        self.cursor.head_pos = pos;
        self.updateCol();
    }

    fn goCol(self: *Editor, col: usize) void {
        const line_start = self.lineStart();
        self.cursor.head_col = min(col, self.lineEnd() - line_start);
        self.cursor.head_pos = line_start + self.cursor.head_col;
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

    fn goLeft(self: *Editor) void {
        self.cursor.head_pos -= @as(usize, if (self.cursor.head_pos == 0) 0 else 1);
        self.updateCol();
    }

    fn goRight(self: *Editor) void {
        self.cursor.head_pos += @as(usize, if (self.cursor.head_pos >= self.text.items.len) 0 else 1);
        self.updateCol();
    }

    fn goLineStart(self: *Editor) void {
        self.cursor.head_pos = self.lineStart();
        self.cursor.head_col = 0;
    }

    fn goLineEnd(self: *Editor) void {
        self.cursor.head_pos = self.searchForwards("\n") orelse self.text.items.len;
    }

    fn goPageStart(self: *Editor) void {
        self.goPos(0);
    }

    fn goPageEnd(self: *Editor) void {
        self.goPos(self.text.items.len);
    }

    fn goDown(self: *Editor) void {
        if (self.searchForwards("\n")) |line_end| {
            const col = self.cursor.head_col;
            self.cursor.head_pos = line_end;
            self.goRight();
            self.goCol(col);
            self.cursor.head_col = col;
        }
    }

    fn goUp(self: *Editor) void {
        if (self.searchBackwards("\n")) |line_start| {
            const col = self.cursor.head_col;
            self.cursor.head_pos = line_start;
            self.goLeft();
            self.goCol(col);
            self.cursor.head_col = col;
        }
    }

    fn deleteSelection(self: *Editor) void {
        if (self.selectionPos()) |pos| {
            std.mem.copy(u8, self.text.items[pos[0]..], self.text.items[pos[1]..]);
            self.text.shrink(self.text.items.len - (pos[1] - pos[0]));
            self.cursor.head_pos = pos[0];
            self.cursor.head_col = pos[0];
            self.clearMark();
        }
    }

    fn deleteBackwards(self: *Editor) void {
        if (self.selectionPos()) |_| {
            self.deleteSelection();
        } else if (self.cursor.head_pos > 0) {
            std.mem.copy(u8, self.text.items[self.cursor.head_pos-1..], self.text.items[self.cursor.head_pos..]);
            _ = self.text.pop();
            self.goLeft();
        }
    }

    fn deleteForwards(self: *Editor) void {
        if (self.selectionPos()) |_| {
            self.deleteSelection();
        } else if (self.cursor.head_pos < self.text.items.len) {
            std.mem.copy(u8, self.text.items[self.cursor.head_pos..], self.text.items[self.cursor.head_pos+1..]);
            _ = self.text.pop();
        }
    }

    fn insert(self: *Editor, chars: []const u8) ! void {
        try self.text.resize(self.text.items.len + chars.len);
        std.mem.copyBackwards(u8, self.text.items[self.cursor.head_pos+chars.len..], self.text.items[self.cursor.head_pos..self.text.items.len-chars.len]);
        std.mem.copy(u8, self.text.items[self.cursor.head_pos..], chars);
        self.cursor.head_pos += chars.len;
    }

    fn clearMark(self: *Editor) void {
        self.cursor.tail_pos = null;
    }

    fn setMarkPos(self: *Editor, pos: usize) void {
        self.cursor.tail_pos = pos;
    }

    fn setMark(self: *Editor) void {
        self.cursor.tail_pos = self.cursor.head_pos;
    }

    fn selectionPos(self: *Editor) ?[2]usize {
        if (self.cursor.tail_pos) |tail_pos| {
            const selection_start_pos = min(self.cursor.head_pos, tail_pos);
            const selection_end_pos = max(self.cursor.head_pos, tail_pos);
            return [2]usize{selection_start_pos, selection_end_pos};
        } else {
            return null;
        }
    }

    fn selection(self: *Editor) ?[]const u8 {
        if (self.selectionPos()) |pos| {
            return self.text.items[pos[0]..pos[1]];
        } else {
            return null;
        }
    }

    fn copy(self: *Editor) ! void {
        if (self.clipboard) |clipboard| {
            self.allocator.free(clipboard);
        }
        self.clipboard = try std.mem.dupe(self.allocator, u8, self.selection() orelse "");
        self.clearMark();
    }

    fn cut(self: *Editor) ! void {
        if (self.clipboard) |clipboard| {
            self.allocator.free(clipboard);
        }
        self.clipboard = try std.mem.dupe(self.allocator, u8, self.selection() orelse "");
        self.deleteSelection();
    }

    fn paste(self: *Editor) ! void {
        try self.insert(self.clipboard orelse "");
    }

    pub fn frame(self: *Editor, ui: *UI, rect: UI.Rect) ! void {
        for (ui.key_went_down.items) |key| {
            dump(key);
            const ctrl = 224;
            const alt = 226;
            if (ui.key_is_down[ctrl]) {
                switch (key) {
                    ' ' => self.setMark(),
                    'c' => try self.copy(),
                    'x' => try self.cut(),
                    'v' => try self.paste(),
                    'j' => self.goLeft(),
                    'l' => self.goRight(),
                    'k' => self.goDown(),
                    'i' => self.goUp(),
                    else => {},
                }
            } else if (ui.key_is_down[alt]) {
                switch (key) {
                    ' ' => self.clearMark(),
                    'j' => self.goLineStart(),
                    'l' => self.goLineEnd(),
                    'k' => self.goPageEnd(),
                    'i' => self.goPageStart(),
                    else => {},
                }
            } else {
                switch (key) {
                    8 => self.deleteBackwards(),
                    13 => try self.insert(&[1]u8{'\n'}),
                    79 => self.goRight(),
                    80 => self.goLeft(),
                    81 => self.goDown(),
                    82 => self.goUp(),
                    127 => self.deleteForwards(),
                    else => if (key >= 32 and key <= 126) {
                        try self.insert(&[1]u8{key});
                    }
                }
            }
        }

        if (ui.mouse_is_down[0]) {
            const line = @divTrunc(ui.mouse_pos.y - rect.y, atlas.text_height);
            const col = @divTrunc(ui.mouse_pos.x - rect.x, atlas.max_char_width);
            self.goLineCol(line, col);
            if (ui.mouse_went_down[0]) {
                self.clearMark();
                self.mouse_went_down_at = self.cursor;
            } else if (self.cursor.head_pos != self.mouse_went_down_at.head_pos) {
                self.setMarkPos(self.mouse_went_down_at.head_pos);
            }   
        }

        const text_color = UI.Color{ .r = 255, .g = 255, .b = 255, .a = 255 };
        const highlight_color = UI.Color{ .r = 255, .g = 255, .b = 255, .a = 100 };
        
        var lines = std.mem.split(self.text.items, "\n");
        var line_ix: u16 = 0;
        var line_start_pos: usize = 0;
        const selection_start_pos = min(self.cursor.head_pos, self.cursor.tail_pos orelse self.cursor.head_pos);
        const selection_end_pos = max(self.cursor.head_pos, self.cursor.tail_pos orelse self.cursor.head_pos);
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
