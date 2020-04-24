usingnamespace @import("common.zig");

const atlas = @import("./atlas.zig");
const draw = @import("./draw.zig");

pub const Fui = struct {
    key: ?u8,
    clip_stack: ArrayList(Rect),
    command_queue: ArrayList(Command),

    pub const Coord = draw.Coord;
    pub const Color = draw.Color;
    pub const Vec2 = draw.Vec2;
    pub const Rect = draw.Rect;

    pub const Command = union(enum){
        Clip: Rect,
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

    pub fn init(allocator: *Allocator) !Fui {
        var clip_stack = ArrayList(Rect).init(allocator);
        try clip_stack.append(Rect{.x=0, .y=0, .w=draw.screen_width, .h=draw.screen_height});
        return Fui{
            .key = null,
            .clip_stack = clip_stack,
            .command_queue = ArrayList(Command).init(allocator),
        };
    }

    pub fn deinit(self: *Fui) void {
        // TODO
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

    pub fn begin(self: *Fui) !void {
        assert(self.clip_stack.items.len == 1);
        assert(self.command_queue.items.len == 0);
    }

    pub fn end(self: *Fui) !void {
        assert(self.clip_stack.items.len == 1);
        draw.clear(.{.r=0, .g=0, .b=0, .a=255});
        for (self.command_queue.items) |command| {
            switch (command) {
                .Clip => |data| draw.set_clip(data),
                .Rect => |data| draw.rect(data.rect, data.color),
                .Text => |data| draw.text(data.chars, data.pos, data.color),
            }
        }
        draw.swap();
        try self.command_queue.resize(0);
    }

    fn intersect_rects(rect1: Rect, rect2: Rect) Rect {
        const x1 = max(rect1.x, rect2.x);
        const y1 = max(rect1.y, rect2.y);
        var x2 = min(rect1.x + rect1.w, rect2.x + rect2.w);
        var y2 = min(rect1.y + rect1.h, rect2.y + rect2.h);
        if (x2 < x1) { x2 = x1; }
        if (y2 < y1) { y2 = y1; }
        return Rect{.x=x1, .y=y1, .w=x2-x1, .h=y2-y1};
    }

    fn peek_clip(self: *Fui) Rect {
        return self.clip_stack.items[self.clip_stack.items.len-1];
    }

    pub fn push_clip(self: *Fui, rect: Rect) !void {
        try self.clip_stack.append(intersect_rects(rect, self.peek_clip()));
    }

    pub fn pop_clip(self: *Fui) void {
        self.clip_stack.pop();
    }

    pub fn text(self: *Fui, chars: str, pos: Vec2, color: Color) !void {
        const clip = self.peek_clip();
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
                    if (w > clip.w) {
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
                .pos = .{.x=clip.x, .y=clip.y+h},
                .color = color,
                .chars = chars[line_begin..line_end],
            }});
            line_begin = line_end;
            h += atlas.text_height;
            if (line_begin >= chars.len or h > clip.h) { break; }
        }
    }
};
