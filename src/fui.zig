usingnamespace @import("common.zig");

const atlas = @import("./atlas.zig");
const draw = @import("./draw.zig");

pub const Fui = struct {
    key: ?u8,
    mouse_pos: Vec2,
    mouse_down: [3]bool,
    command_queue: ArrayList(Command),

    pub const Coord = draw.Coord;
    pub const Color = draw.Color;
    pub const Vec2 = draw.Vec2;
    pub const Rect = draw.Rect;

    pub const Command = union(enum){
        Rect: struct {
            rect: Rect,
            color: Color,
        },
        Text: struct {
            pos: Vec2,
            color: draw.Color,
            chars: str,
        },
    };

    pub fn init(allocator: *Allocator) Fui {
        return Fui{
            .key = null,
            .mouse_pos = .{.x=0,.y=0},
            .mouse_down = .{false,false,false},
            .command_queue = ArrayList(Command).init(allocator),
        };
    }

    pub fn deinit(self: *Fui) void {
        self.command_queue.deinit();
    }

    pub fn handleInput(self: *Fui) bool {
        var got_input = false;
        var e: c.SDL_Event = undefined;
        while (c.SDL_PollEvent(&e) != 0) {
            got_input = true;
            switch (e.type) {
                c.SDL_QUIT => std.os.exit(0),
                c.SDL_KEYDOWN, c.SDL_KEYUP => {
                    switch (e.type) {
                        c.SDL_KEYDOWN => self.key = @intCast(u8, e.key.keysym.sym & 0xff),
                        c.SDL_KEYUP => self.key = null,
                        else => unreachable,
                    }
                },
                c.SDL_MOUSEMOTION => {
                    self.mouse_pos = .{.x=@intCast(u16, e.motion.x * draw.scale), .y=@intCast(u16, e.motion.y * draw.scale)};
                },
                c.SDL_MOUSEBUTTONDOWN => {
                    switch (e.button.button) {
                        c.SDL_BUTTON_LEFT => self.mouse_down[0] = true,
                        c.SDL_BUTTON_MIDDLE => self.mouse_down[1] = true,
                        c.SDL_BUTTON_RIGHT => self.mouse_down[2] = true,
                        else => {}
                    }
                },
                c.SDL_MOUSEBUTTONUP => {
                      switch (e.button.button) {
                        c.SDL_BUTTON_LEFT => self.mouse_down[0] = false,
                        c.SDL_BUTTON_MIDDLE => self.mouse_down[1] = false,
                        c.SDL_BUTTON_RIGHT => self.mouse_down[2] = false,
                        else => {}
                    }
                },
                else => {}
            }
        }
        return got_input;
    }

    pub fn begin(self: *Fui) !Rect {
        assert(self.command_queue.items.len == 0);
        return Rect{.x=0,.y=0,.w=draw.screen_width,.h=draw.screen_height};
    }

    pub fn end(self: *Fui) !void {
        draw.clear(.{.r=0, .g=0, .b=0, .a=255});
        for (self.command_queue.items) |command| {
            switch (command) {
                .Rect => |data| draw.rect(data.rect, data.color),
                .Text => |data| draw.text(data.chars, data.pos, data.color),
            }
        }
        draw.swap();
        try self.command_queue.resize(0);
    }

    fn queueRect(self: *Fui, rect: Rect, color: Color) !void {
        try self.command_queue.append(.{.Rect = .{
            .rect = rect,
            .color = color,
        }});
    }

    fn queueText(self: *Fui, pos: Vec2, color: Color, chars: str) !void {
        try self.command_queue.append(.{.Text = .{
            .pos = pos,
            .color = color,
            .chars = chars,
        }});
    }

    pub fn text(self: *Fui, rect: Rect, chars: str, color: Color) !void {
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
                    w += @intCast(Coord, atlas.charWidth(char));
                    if (w > rect.w) {
                        // if haven't soft wrapped yet, hard wrap before this char
                        if (line_end == line_begin) {
                            line_end = i;
                        }
                        break;
                    }
                    if (char == '\n') {
                        // commit to drawing this char and wrap here
                        line_end = i+1;
                        break;
                    }
                    if (char == ' ') {
                        // commit to drawing this char
                        line_end = i+1;
                    }
                    // otherwise keep looking ahead
                    i += 1;
                }
            }
            try self.queueText(.{.x=rect.x, .y=rect.y+h}, color, chars[line_begin..line_end]);
            line_begin = line_end;
            h += atlas.text_height;
            if (line_begin >= chars.len or h > rect.h) { break; }
        }
    }

    pub fn isMouseHere(self: *Fui, rect: Rect) bool {
        return
            self.mouse_pos.x >= rect.x and
            self.mouse_pos.x < rect.x + rect.w and
            self.mouse_pos.y >= rect.y and
            self.mouse_pos.y < rect.y + rect.h;
    }

    pub fn isMouseDown(self: *Fui, rect: Rect) bool {
        return
            self.mouse_down[0] and
            self.isMouseHere(rect);
    }

    pub fn button(self: *Fui, rect: Rect, chars: str, color: Color) !bool {
        const down = self.isMouseDown(rect);
        try self.queueRect(rect, color);
        if (!down) {
            try self.queueRect(.{.x=rect.x+atlas.scale, .y=rect.y+atlas.scale, .w=subSaturating(Coord, rect.w, 2*atlas.scale), .h=subSaturating(Coord, rect.h, 2*atlas.scale)},
                                .{.r=0, .g=0, .b=0, .a=255});
        }
        try self.text(rect, chars, color);
        return down;
    }
};
