const std = @import("std");
const focus = @import("../focus.zig");
const freetype = @import("freetype");
const u = focus.util;
const c = focus.util.c;

const fira_code = @embedFile("../../fonts/Fira_Code_v5.2/woff/FiraCode-Regular.woff");

pub const Atlas = struct {
    allocator: u.Allocator,
    char_size_pixels: usize,
    texture: []u.Color,
    texture_dims: u.Vec2,
    char_width: u.Coord,
    char_height: u.Coord,
    char_to_rect: []u.Rect,
    white_rect: u.Rect,

    pub fn init(allocator: u.Allocator, char_size_pixels: usize) Atlas {
        // init freetype
        const lib = freetype.Library.init() catch |err|
            u.panic("Error initializing freetype: {}", .{err});
        defer lib.deinit();

        // load font
        const face = lib.newFaceMemory(fira_code, 0) catch |err|
            u.panic("Error loading font: {}", .{err});
        defer face.deinit();

        // set font size
        face.setPixelSizes(@intCast(u32, char_size_pixels), @intCast(u32, char_size_pixels)) catch |err|
            u.panic("Error setting font size: {}", .{err});

        // render every ascii char
        const num_chars: usize = 128;
        const char_to_bitmap = allocator.alloc([]const u.Color, num_chars) catch u.oom();
        const char_to_cbox = allocator.alloc(freetype.BBox, num_chars) catch u.oom();
        {
            var char: usize = 0;
            while (char < num_chars) : (char += 1) {
                face.loadChar(@intCast(u32, char), .{ .render = true }) catch |err|
                    u.panic("Error loading '{}': {}", .{ char, err });

                const bitmap = face.glyph.bitmap();
                const bitmap_width = bitmap.width();
                const bitmap_height = bitmap.rows();
                const bitmap_pitch = bitmap.pitch();
                const buffer = bitmap.buffer();
                const bitmap_copy = allocator.alloc(u.Color, bitmap_width * bitmap_height) catch u.oom();
                var x: usize = 0;
                while (x < bitmap_width) : (x += 1) {
                    var y: usize = 0;
                    while (y < bitmap_height) : (y += 1) {
                        bitmap_copy[(y * bitmap_width) + x] = .{
                            .r = 255,
                            .g = 255,
                            .b = 255,
                            .a = buffer[(y * @intCast(usize, bitmap_pitch)) + x],
                        };
                    }
                }

                const glyph = face.glyph.glyph() catch |err|
                    u.panic("Error getting glyph for '{}': {}", .{ char, err });
                const cbox = glyph.getCBox(.pixels);
                u.assert(cbox.yMax == face.glyph.bitmapTop());
                u.assert(cbox.yMin == face.glyph.bitmapTop() - @intCast(i32, bitmap.rows()));
                u.assert(cbox.xMin == face.glyph.bitmapLeft());
                u.assert(cbox.xMax == face.glyph.bitmapLeft() + @intCast(i32, bitmap.width()));

                char_to_bitmap[char] = bitmap_copy;
                char_to_cbox[char] = cbox;
            }
        }
        defer {
            for (char_to_bitmap) |bitmap| allocator.free(bitmap);
            allocator.free(char_to_bitmap);
            allocator.free(char_to_cbox);
        }

        // figure out how much space we need per character
        var max_cbox = freetype.BBox{ .xMin = 0, .yMin = 0, .xMax = 0, .yMax = 0 };
        for (char_to_cbox) |cbox| {
            max_cbox.xMin = u.min(max_cbox.xMin, cbox.xMin);
            max_cbox.yMin = u.min(max_cbox.yMin, cbox.yMin);
            max_cbox.xMax = u.max(max_cbox.xMax, cbox.xMax);
            max_cbox.yMax = u.max(max_cbox.yMax, cbox.yMax);
        }
        const char_width = @intCast(u.Coord, max_cbox.xMax - max_cbox.xMin);
        const char_height = @intCast(u.Coord, max_cbox.yMax - max_cbox.yMin);

        // copy every char into a single texture
        const texture = allocator.alloc(u.Color, num_chars * @intCast(usize, char_width * char_height)) catch u.oom();
        for (texture) |*pixel| pixel.* = .{ .r = 0, .g = 0, .b = 0, .a = 0 };
        const char_to_rect = allocator.alloc(u.Rect, num_chars) catch u.oom();
        for (char_to_bitmap) |bitmap, char| {
            const cbox = char_to_cbox[char];

            const bitmap_width = cbox.xMax - cbox.xMin;
            const bitmap_height = cbox.yMax - cbox.yMin;

            var by: i32 = 0;
            while (by < bitmap_height) : (by += 1) {
                var bx: i32 = 0;
                while (bx < bitmap_width) : (bx += 1) {
                    // TODO use offsets here
                    const tx = (@intCast(u.Coord, char) * char_width) + bx;
                    const ty = by;
                    const ti = @intCast(usize, (ty * @intCast(u.Coord, num_chars) * char_width) + tx);
                    const bi = @intCast(usize, (by * bitmap_width) + bx);
                    texture[ti] = bitmap[bi];
                }
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
            .char_size_pixels = char_size_pixels,
            .texture = texture,
            .texture_dims = .{
                .x = @intCast(u.Coord, num_chars) * char_width,
                .y = char_height,
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
