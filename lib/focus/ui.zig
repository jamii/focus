const focus = @import("../focus.zig");
usingnamespace focus.common;
const atlas = focus.atlas;
const draw = focus.draw;

pub const UI = struct {
    key: ?u8,
    key_went_down: bool,
    mouse_pos: Vec2,
    mouse_is_down: [3]bool,
    mouse_went_down: [3]bool,
    command_queue: ArrayList(Command),

    pub const Coord = draw.Coord;
    pub const Color = draw.Color;
    pub const Vec2 = draw.Vec2;
    pub const Rect = draw.Rect;

    pub const Command = union(enum) {
        Rect: struct {
            rect: Rect,
            color: Color,
        },
        Text: struct {
            pos: Vec2,
            color: draw.Color,
            chars: []const u8,
        },
    };

    pub fn init(allocator: *Allocator) UI {
        return UI{
            .key = null,
            .key_went_down = false,
            .mouse_pos = .{ .x = 0, .y = 0 },
            .mouse_is_down = .{ false, false, false },
            .mouse_went_down = .{ false, false, false },
            .command_queue = ArrayList(Command).init(allocator),
        };
    }

    pub fn deinit(self: *UI) void {
        self.command_queue.deinit();
    }

    pub fn handleInput(self: *UI) bool {
        var got_input = false;
        self.key_went_down = false;
        self.mouse_went_down = .{ false, false, false };
        var e: c.SDL_Event = undefined;
        while (c.SDL_PollEvent(&e) != 0) {
            got_input = true;
            switch (e.type) {
                c.SDL_QUIT => std.os.exit(0),
                c.SDL_KEYDOWN, c.SDL_KEYUP => {
                    switch (e.type) {
                        c.SDL_KEYDOWN => {
                            self.key = @intCast(u8, e.key.keysym.sym & 0xff);
                            self.key_went_down = true;
                        },
                        c.SDL_KEYUP => {
                            assert(!self.key_went_down); // can we get down and up in a single frame?
                            self.key = null;
                        },
                        else => unreachable,
                    }
                },
                c.SDL_MOUSEMOTION => {
                    self.mouse_pos = .{ .x = @intCast(u16, e.motion.x * draw.scale), .y = @intCast(u16, e.motion.y * draw.scale) };
                },
                c.SDL_MOUSEBUTTONDOWN => {
                    switch (e.button.button) {
                        c.SDL_BUTTON_LEFT => {
                            self.mouse_is_down[0] = true;
                            self.mouse_went_down[0] = true;
                        },
                        c.SDL_BUTTON_MIDDLE => {
                            self.mouse_is_down[1] = true;
                            self.mouse_went_down[1] = true;
                        },
                        c.SDL_BUTTON_RIGHT => {
                            self.mouse_is_down[2] = true;
                            self.mouse_went_down[2] = true;
                        },
                        else => {},
                    }
                },
                c.SDL_MOUSEBUTTONUP => {
                    switch (e.button.button) {
                        c.SDL_BUTTON_LEFT => self.mouse_is_down[0] = false,
                        c.SDL_BUTTON_MIDDLE => self.mouse_is_down[1] = false,
                        c.SDL_BUTTON_RIGHT => self.mouse_is_down[2] = false,
                        else => {},
                    }
                },
                else => {},
            }
        }
        return got_input;
    }

    pub fn begin(self: *UI) !Rect {
        assert(self.command_queue.items.len == 0);
        return Rect{ .x = 0, .y = 0, .w = draw.screen_width, .h = draw.screen_height };
    }

    pub fn end(self: *UI) !void {
        draw.clear(.{ .r = 0, .g = 0, .b = 0, .a = 255 });
        for (self.command_queue.items) |command| {
            switch (command) {
                .Rect => |data| draw.rect(data.rect, data.color),
                .Text => |data| draw.text(data.chars, data.pos, data.color),
            }
        }
        draw.swap();
        try self.command_queue.resize(0);
    }

    fn queueRect(self: *UI, rect: Rect, color: Color) !void {
        try self.command_queue.append(.{
            .Rect = .{
                .rect = rect,
                .color = color,
            },
        });
    }

    fn queueText(self: *UI, pos: Vec2, color: Color, chars: []const u8) !void {
        try self.command_queue.append(.{
            .Text = .{
                .pos = pos,
                .color = color,
                .chars = chars,
            },
        });
    }

    pub fn text(self: *UI, rect: Rect, color: Color, chars: []const u8) !void {
        var h: Coord = 0;
        var line_begin: usize = 0;
        while (true) {
            var line_end = line_begin;
            {
                var w: Coord = 0;
                var i: usize = line_end;
                while (true) {
                    if (i >= chars.len) {
                        line_end = i;
                        break;
                    }
                    const char = chars[i];
                    w += @intCast(Coord, atlas.max_char_width);
                    if (w > rect.w) {
                        // if haven't soft wrapped yet, hard wrap before this char
                        if (line_end == line_begin) {
                            line_end = i;
                        }
                        break;
                    }
                    if (char == '\n') {
                        // commit to drawing this char and wrap here
                        line_end = i + 1;
                        break;
                    }
                    if (char == ' ') {
                        // commit to drawing this char
                        line_end = i + 1;
                    }
                    // otherwise keep looking ahead
                    i += 1;
                }
            }
            try self.queueText(.{ .x = rect.x, .y = rect.y + h }, color, chars[line_begin..line_end]);
            line_begin = line_end;
            h += atlas.text_height;
            if (line_begin >= chars.len or h > rect.h) {
                break;
            }
        }
    }

    pub fn mouseIsHere(self: *UI, rect: Rect) bool {
        return self.mouse_pos.x >= rect.x and
            self.mouse_pos.x < rect.x + rect.w and
            self.mouse_pos.y >= rect.y and
            self.mouse_pos.y < rect.y + rect.h;
    }

    pub fn mouseIsDown(self: *UI, rect: Rect) bool {
        return self.mouse_is_down[0] and
            self.mouseIsHere(rect);
    }

    pub fn mouseWentDown(self: *UI, rect: Rect) bool {
        return self.mouse_went_down[0] and
            self.mouseIsHere(rect);
    }

    pub fn keyIsDown(self: *UI, key: u8) bool {
        return (self.key orelse 0) == key;
    }

    pub fn keyWentDown(self: *UI, key: u8) bool {
        return self.keyIsDown(key) and self.key_went_down;
    }

    pub fn buttonHeight(margin: Coord) Coord {
        return atlas.text_height + (margin * 4);
    }

    pub fn button(self: *UI, rect: Rect, color: Color, margin: Coord, chars: []const u8) !bool {
        try self.queueRect(rect, color);
        if (!self.mouseIsDown(rect)) {
            try self.queueRect(rect.shrink(margin), .{ .r = 0, .g = 0, .b = 0, .a = 255 });
        }
        var text_rect = rect.shrink(margin*2);
        text_rect.x += @divTrunc(text_rect.w - @intCast(u16, atlas.textWidth(chars)), 2);
        try self.text(text_rect, color, chars);
        return self.mouseWentDown(rect);
    }
};
