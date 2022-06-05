const std = @import("std");
const focus = @import("../focus.zig");
const freetype = @import("freetype");
const u = focus.util;
const c = focus.util.c;

const fira_code = @embedFile("../../fonts/Fira_Code_v5.2/woff/FiraCode-Regular.woff");

pub const Atlas = struct {
    allocator: u.Allocator,
    point_size: usize,
    texture: []u.Color,
    texture_dims: u.Vec2,
    char_width: u.Coord,
    char_height: u.Coord,
    char_to_rect: []u.Rect,
    white_rect: u.Rect,

    pub fn init(allocator: u.Allocator, point_size: usize) Atlas {

        // init freetype
        const lib = try freetype.Library.init();
        defer lib.deinit();

        // load font
        const face = try lib.newFaceMemory(fira_code, 0);
        defer face.deinit();

        // set font size
        try face.setCharSize(40 * 64, 0, 50, 0);

        // TODO ???
        const num_chars: usize = 128;
        const char_width: u.Coord = 32;
        const char_height: u.Coord = 24;

        // going to draw everything into this texture
        const texture = allocator.alloc(u.Color, @intCast(usize, num_chars * char_width * char_height)) catch u.oom();
        const texture_width = num_chars * char_width;

        // going to store the coordinates of each char within the texture here
        var char_to_rect = allocator.alloc(u.Rect, num_chars) catch u.oom();

        // render every ascii char
        var char: u8 = 0;
        while (char < num_chars) : (char += 1) {
            try face.loadChar(char, .{ .render = true });
            const glyph = face.glyph;
            const x = @intCast(usize, glyph.bitmapLeft());
            const y = char_height - @intCast(usize, glyph.bitmapTop());
            const bitmap = glyph.bitmap();
            var p: usize = 0;
            var q: usize = 0;
            const w = bitmap.width();
            const x_max = x + w;
            const y_max = y + bitmap.rows();
            var i: usize = 0;

            while (i < x_max - x) : (i += 1) {
                var j: usize = 0;
                while (j < y_max - y) : (j += 1) {
                    if (i < char_width and j < char_height) {
                        texture[(j * texture_width) + (char * char_width) + i] |= bitmap.buffer()[q * w + p];
                        q += 1;
                    }
                }
                q = 0;
                p += 1;
            }

            char_to_rect[char] = .{
                .x = @intCast(u.Coord, char) * char_width,
                .y = 0,
                .w = char_width,
                .h = char_height,
            };
        }

        // make a white pixel
        texture[0] = u.Color{ .r = 255, .g = 255, .b = 255, .a = 255 };
        const white_rect = u.Rect{ .x = 0, .y = 0, .w = 1, .h = 1 };

        return Atlas{
            .allocator = allocator,
            .point_size = point_size,
            .texture = texture,
            .texture_dims = .{
                .x = texture_width,
                .y = char_height,
            },
            .char_width = char_width,
            .char_height = char_height,
            .char_to_rect = char_to_rect,
            .white_rect = white_rect,
        };
    }

    pub fn deinit(self: *Atlas) void {
        self.allocator.free(self.char_to_rect);
        self.allocator.free(self.texture);
    }
};
