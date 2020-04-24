usingnamespace @import("common.zig");

const atlas = @import("./atlas.zig");
const draw = @import("./draw.zig");

pub const Fui = struct {
    key: ?u8,
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
            .command_queue = ArrayList(Command).init(allocator),
        };
    }

    pub fn deinit(self: *Fui) void {
        self.command_queue.deinit();
    }

    pub fn handle_input(self: *Fui) bool {
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
                    w += @intCast(Coord, atlas.char_width(char));
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
            try self.command_queue.append(.{.Text = .{
                .pos = .{.x=rect.x, .y=rect.y+h},
                .color = color,
                .chars = chars[line_begin..line_end],
            }});
            line_begin = line_end;
            h += atlas.text_height;
            if (line_begin >= chars.len or h > rect.h) { break; }
        }
    }
};
