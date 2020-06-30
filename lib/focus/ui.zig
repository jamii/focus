const focus = @import("../focus.zig");
usingnamespace focus.common;
const draw = focus.draw;
const Atlas = focus.Atlas;

pub const UI = struct {
    allocator: *Allocator,
    atlas: *Atlas,
    events: ArrayList(c.SDL_Event),
    commands: ArrayList(Command),

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

    pub fn init(allocator: *Allocator, atlas: *Atlas) UI {
        return UI{
            .allocator = allocator,
            .atlas = atlas,
            .events = ArrayList(c.SDL_Event).init(allocator),
            .commands = ArrayList(Command).init(allocator),
        };
    }

    pub fn deinit(self: *UI) void {
        self.events.deinit();
        self.commands.deinit();
    }

    pub fn handleInput(self: *UI) ! void {
        var got_input = false;
        var event: c.SDL_Event = undefined;
        while (c.SDL_PollEvent(&event) != 0) {
            if (event.type == c.SDL_QUIT) {
                std.os.exit(0);
            }
            try self.events.append(event);
        }
    }

    pub fn begin(self: *UI) !Rect {
        assert(self.commands.items.len == 0);
        return Rect{ .x = 0, .y = 0, .w = draw.screen_width, .h = draw.screen_height };
    }

    pub fn end(self: *UI) !void {
        draw.clear(.{ .r = 0, .g = 0, .b = 0, .a = 255 });
        for (self.commands.items) |command| {
            switch (command) {
                .Rect => |data| draw.rect(self.atlas, data.rect, data.color),
                .Text => |data| draw.text(self.atlas, data.chars, data.pos, data.color),
            }
        }
        draw.swap();
        try self.events.resize(0);
        try self.commands.resize(0);
    }

    fn queueRect(self: *UI, rect: Rect, color: Color) !void {
        try self.commands.append(.{
            .Rect = .{
                .rect = rect,
                .color = color,
            },
        });
    }

    fn queueText(self: *UI, pos: Vec2, color: Color, chars: []const u8) !void {
        try self.commands.append(.{
            .Text = .{
                .pos = pos,
                .color = color,
                .chars = chars,
            },
        });
    }

    // pub fn text(self: *UI, rect: Rect, color: Color, chars: []const u8) !void {
    //     var h: Coord = 0;
    //     var line_begin: usize = 0;
    //     while (true) {
    //         var line_end = line_begin;
    //         {
    //             var w: Coord = 0;
    //             var i: usize = line_end;
    //             while (true) {
    //                 if (i >= chars.len) {
    //                     line_end = i;
    //                     break;
    //                 }
    //                 const char = chars[i];
    //                 w += @intCast(Coord, atlas.max_char_width);
    //                 if (w > rect.w) {
    //                     // if haven't soft wrapped yet, hard wrap before this char
    //                     if (line_end == line_begin) {
    //                         line_end = i;
    //                     }
    //                     break;
    //                 }
    //                 if (char == '\n') {
    //                     // commit to drawing this char and wrap here
    //                     line_end = i + 1;
    //                     break;
    //                 }
    //                 if (char == ' ') {
    //                     // commit to drawing this char
    //                     line_end = i + 1;
    //                 }
    //                 // otherwise keep looking ahead
    //                 i += 1;
    //             }
    //         }
    //         try self.queueText(.{ .x = rect.x, .y = rect.y + h }, color, chars[line_begin..line_end]);
    //         line_begin = line_end;
    //         h += atlas.text_height;
    //         if (line_begin >= chars.len or h > rect.h) {
    //             break;
    //         }
    //     }
    // }
};
