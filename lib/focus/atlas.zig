const std = @import("std");
const focus = @import("../focus.zig");
const freetype = @import("mach-freetype");
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
        const face = lib.createFaceMemory(fira_code, 0) catch |err|
            u.panic("Error loading font: {}", .{err});
        defer face.deinit();

        // set font size
        face.setPixelSizes(@intCast(char_size_pixels), @intCast(char_size_pixels)) catch |err|
            u.panic("Error setting font size: {}", .{err});

        // render every ascii char
        const num_ascii_chars: usize = 128;
        const char_to_bitmap = allocator.alloc([]const u.Color, num_ascii_chars) catch u.oom();
        const char_to_cbox = allocator.alloc(freetype.BBox, num_ascii_chars) catch u.oom();
        {
            var char: usize = 0;
            while (char < num_ascii_chars) : (char += 1) {
                face.loadChar(@intCast(char), .{ .render = true }) catch |err|
                    u.panic("Error loading '{}': {}", .{ char, err });

                const bitmap = face.glyph().bitmap();
                const bitmap_width = bitmap.width();
                const bitmap_height = bitmap.rows();
                const bitmap_pitch = bitmap.pitch();
                const bitmap_copy = allocator.alloc(u.Color, bitmap_width * bitmap_height) catch u.oom();
                if (bitmap.buffer()) |buffer| {
                    var x: usize = 0;
                    while (x < bitmap_width) : (x += 1) {
                        var y: usize = 0;
                        while (y < bitmap_height) : (y += 1) {
                            bitmap_copy[(y * bitmap_width) + x] = .{
                                .r = 255,
                                .g = 255,
                                .b = 255,
                                .a = buffer[(y * @as(usize, @intCast(bitmap_pitch))) + x],
                            };
                        }
                    }
                }

                const glyph = face.glyph().getGlyph() catch |err|
                    u.panic("Error getting glyph for '{}': {}", .{ char, err });
                const cbox = glyph.getCBox(.pixels);
                u.assert(cbox.yMax == face.glyph().bitmapTop());
                u.assert(cbox.yMin == face.glyph().bitmapTop() - @as(i32, @intCast(bitmap.rows())));
                u.assert(cbox.xMin == face.glyph().bitmapLeft());
                u.assert(cbox.xMax == face.glyph().bitmapLeft() + @as(i32, @intCast(bitmap.width())));

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
            max_cbox.xMin = @min(max_cbox.xMin, cbox.xMin);
            max_cbox.yMin = @min(max_cbox.yMin, cbox.yMin);
            max_cbox.xMax = @max(max_cbox.xMax, cbox.xMax);
            max_cbox.yMax = @max(max_cbox.yMax, cbox.yMax);
        }
        const char_width = @as(u.Coord, @intCast(max_cbox.xMax - max_cbox.xMin));
        const char_height = @as(u.Coord, @intCast(max_cbox.yMax - max_cbox.yMin));

        // copy every char into a single texture
        const num_chars = num_ascii_chars + 1; // ascii + one white box
        const texture = allocator.alloc(u.Color, num_chars * @as(usize, @intCast(char_width * char_height))) catch u.oom();
        for (texture) |*pixel| pixel.* = .{ .r = 0, .g = 0, .b = 0, .a = 0 };
        const char_to_rect = allocator.alloc(u.Rect, num_chars) catch u.oom();
        for (char_to_bitmap, 0..) |bitmap, char| {
            const cbox = char_to_cbox[char];

            const bitmap_width = cbox.xMax - cbox.xMin;
            const bitmap_height = cbox.yMax - cbox.yMin;

            var by: i32 = 0;
            while (by < bitmap_height) : (by += 1) {
                var bx: i32 = 0;
                while (bx < bitmap_width) : (bx += 1) {
                    const tx = (@as(u.Coord, @intCast(char)) * char_width) + bx + (cbox.xMin - max_cbox.xMin);
                    const ty = by + (max_cbox.yMax - cbox.yMax);
                    const ti = @as(usize, @intCast((ty * @as(u.Coord, @intCast(num_chars)) * char_width) + tx));
                    const bi = @as(usize, @intCast((by * bitmap_width) + bx));
                    texture[ti] = bitmap[bi];
                }
            }

            char_to_rect[char] = .{
                .x = @as(u.Coord, @intCast(char)) * char_width,
                .y = 0,
                .w = char_width,
                .h = char_height,
            };
        }

        // make a white pixel
        const white_rect = u.Rect{ .x = @as(u.Coord, @intCast(num_ascii_chars)) * char_width, .y = 0, .w = 1, .h = 1 };
        texture[@intCast(white_rect.x)] = u.Color{ .r = 255, .g = 255, .b = 255, .a = 255 };

        return Atlas{
            .allocator = allocator,
            .char_size_pixels = char_size_pixels,
            .texture = texture,
            .texture_dims = .{
                .x = @as(u.Coord, @intCast(num_chars)) * char_width,
                .y = char_height,
            },
            .char_width = @intCast(char_width),
            .char_height = @intCast(char_height),
            .char_to_rect = char_to_rect,
            .white_rect = white_rect,
        };
    }

    pub fn deinit(self: *Atlas) void {
        self.allocator.free(self.char_to_rect);
        self.allocator.free(self.texture);
    }
};
