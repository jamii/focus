const std = @import("std");
const focus = @import("../focus.zig");
const freetype = @import("freetype");
const u = focus.util;
const c = focus.util.c;

const fira_code = @embedFile("../../fonts/Fira_Code_v5.2/woff/FiraCode-Regular.woff");

pub const Atlas = struct {
    allocator: u.Allocator,
    char_size: usize,
    texture: []u.Color,
    texture_dims: u.Vec2,
    char_width: u.Coord,
    char_height: u.Coord,
    char_to_rect: []u.Rect,
    white_rect: u.Rect,

    pub fn init(allocator: u.Allocator, char_size: usize) Atlas {

        // init freetype
        const lib = freetype.Library.init() catch |err|
            u.panic("Error initializing freetype: {}", .{err});
        defer lib.deinit();

        // load font
        const face = lib.newFaceMemory(fira_code, 0) catch |err|
            u.panic("Error loading font: {}", .{err});
        defer face.deinit();

        // set font size
        face.setPixelSizes(@intCast(u32, char_size), @intCast(u32, char_size)) catch |err|
            u.panic("Error setting font size: {}", .{err});
        const char_width = char_size;
        const char_height = char_size;

        // TODO ???
        const num_chars: usize = 128;

        // going to draw everything into this texture
        const texture = allocator.alloc(u.Color, @intCast(usize, num_chars * char_width * char_height)) catch u.oom();
        for (texture) |*pixel|
            pixel.* = .{ .r = 0, .g = 0, .b = 0, .a = 0 };
        const texture_width = num_chars * char_width;

        // going to store the coordinates of each char within the texture here
        var char_to_rect = allocator.alloc(u.Rect, num_chars) catch u.oom();

        // render every ascii char
        var char: u8 = 0;
        while (char < num_chars) : (char += 1) {
            face.loadChar(char, .{ .render = true }) catch |err|
                u.panic("Error rendering '{}': {}", .{ char, err });
            const glyph = face.glyph;

            const bitmap = glyph.bitmap();
            var p: usize = 0;
            var q: usize = 0;
            const w = bitmap.width();
            const h = bitmap.rows();
            var i: usize = 0;
            while (i < w) : (i += 1) {
                var j: usize = 0;
                while (j < h) : (j += 1) {
                    if (i < char_width and j < char_height) {
                        //const x = @intCast(usize, @intCast(i32, i) + glyph.bitmapLeft());
                        //const y = @intCast(usize, @intCast(i32, j) + glyph.bitmapTop());
                        texture[(j * texture_width) + (char * char_width) + i].a |= bitmap.buffer()[q * w + p];
                        q += 1;
                    }
                }
                q = 0;
                p += 1;
            }

            char_to_rect[char] = .{
                .x = @intCast(u.Coord, char * char_width),
                .y = 0,
                .w = @intCast(u.Coord, char_width),
                .h = @intCast(u.Coord, char_height),
            };
        }

        // make a white pixel
        texture[0] = u.Color{ .r = 255, .g = 255, .b = 255, .a = 255 };
        const white_rect = u.Rect{ .x = 0, .y = 0, .w = 1, .h = 1 };

        return Atlas{
            .allocator = allocator,
            .char_size = char_size,
            .texture = texture,
            .texture_dims = .{
                .x = @intCast(u.Coord, texture_width),
                .y = @intCast(u.Coord, char_height),
            },
            .char_width = @intCast(u.Coord, char_width),
            .char_height = @intCast(u.Coord, char_height),
            .char_to_rect = char_to_rect,
            .white_rect = white_rect,
        };
    }

    pub fn deinit(self: *Atlas) void {
        self.allocator.free(self.char_to_rect);
        self.allocator.free(self.texture);
    }
};
