const focus = @import("../focus.zig");
usingnamespace focus.common;
const atlas = focus.atlas;
const UI = focus.UI;

pub const Editor = struct {
    allocator: *Allocator,
    text: ArrayList(u8),

    pub fn init(allocator: *Allocator, init_text: []const u8) ! Editor {
        var text = try ArrayList(u8).initCapacity(allocator, init_text.len);
        try text.appendSlice(init_text);
        return Editor{.allocator = allocator, .text = text};
    }

    pub fn deinit(self: *Editor) void {
        self.text.deinit();
    }

    const white = UI.Color{ .r = 255, .g = 255, .b = 255, .a = 255 };

    pub fn frame(self: *Editor, ui: *UI, rect: UI.Rect) ! void {
        var lines = std.mem.split(self.text.items, "\n");
        var i: u16 = 0;
        while (lines.next()) |line| : (i += 1) {
            if ((i * atlas.text_height) > rect.h) break;
            try ui.queueText(.{.x = rect.x, .y = rect.y + (i * atlas.text_height)}, white, line);
        }
    }
};
