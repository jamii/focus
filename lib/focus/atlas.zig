const std = @import("std");
const focus = @import("../focus.zig");
const u = focus.util;
const c = focus.util.c;

const fira_code = @embedFile("../../fonts/Fira_Code_v5.2/woff/FiraCode-Regular.woff");

pub const Atlas = struct {
    allocator: u.Allocator,
    point_size: usize,
    font: *c.TTF_Font,
    texture: []u.Color,
    texture_dims: u.Vec2,
    char_width: u.Coord,
    char_height: u.Coord,
    char_to_rect: []u.Rect,
    white_rect: u.Rect,
    right_down_arrow_rect: u.Rect,
    down_right_arrow_rect: u.Rect,

    pub fn init(allocator: u.Allocator, point_size: usize) Atlas {

        // init SDL2_ttf
        if (c.TTF_Init() != 0)
            u.panic("TTF_Init failed: {s}", .{c.TTF_GetError()});

        // load font
        var reader = c.SDL_RWFromConstMem(fira_code, @intCast(c_int, fira_code.len)) orelse u.panic("Font reader failed: {s}", .{c.SDL_GetError()});
        const font = c.TTF_OpenFontRW(
            reader,
            1, // automatically close reader
            @intCast(c_int, point_size),
        ) orelse u.panic("Font load failed: {s}", .{c.TTF_GetError()});

        // text to be rendered
        var text = allocator.allocSentinel(u8, 128 + 6, 0) catch u.oom();
        defer allocator.free(text);

        // going to overwrite this with a white block in final texture
        text[0] = ' ';

        // add all ascii chars
        {
            var char: usize = 1;
            while (char < text.len) : (char += 1) {
                text[char] = @intCast(u8, char);
            }
        }

        // add special characters
        std.mem.copy(u8, text[128..], "⤵⤷");

        // render
        const surface = c.TTF_RenderUTF8_Blended(font, text, c.SDL_Color{ .r = 255, .g = 255, .b = 255, .a = 255 }) orelse u.panic("Atlas render failed: {s}", .{c.TTF_GetError()});
        defer c.SDL_FreeSurface(surface);

        // copy the texture
        u.assert(surface.*.format.*.format == c.SDL_PIXELFORMAT_ARGB8888);
        const texture = allocator.dupe(u.Color, @ptrCast([*]u.Color, surface.*.pixels)[0..@intCast(usize, surface.*.w * surface.*.h)]) catch u.oom();

        // make a white pixel
        texture[0] = u.Color{ .r = 255, .g = 255, .b = 255, .a = 255 };
        const white_rect = u.Rect{ .x = 0, .y = 0, .w = 1, .h = 1 };

        // calculate char sizes
        // assume monospaced font
        const char_width = @intCast(u.Coord, @divTrunc(@intCast(usize, surface.*.w), 128 + 2));
        const char_height = @intCast(u.Coord, surface.*.h);

        // calculate location of each char
        var char_to_rect = allocator.alloc(u.Rect, text.len) catch u.oom();
        char_to_rect[0] = .{ .x = 0, .y = 0, .w = 0, .h = 0 };
        {
            var char: usize = 1;
            while (char < text.len) : (char += 1) {
                char_to_rect[char] = .{
                    .x = @intCast(u.Coord, char) * char_width,
                    .y = 0,
                    .w = char_width,
                    .h = char_height,
                };
            }
        }
        const right_down_arrow_rect = u.Rect{
            .x = 128 * char_width,
            .y = 0,
            .w = char_width,
            .h = char_height,
        };
        const down_right_arrow_rect = u.Rect{
            .x = 129 * char_width,
            .y = 0,
            .w = char_width,
            .h = char_height,
        };

        return Atlas{
            .allocator = allocator,
            .point_size = point_size,
            .font = font,
            .texture = texture,
            .texture_dims = .{
                .x = @intCast(u.Coord, surface.*.w),
                .y = @intCast(u.Coord, surface.*.h),
            },
            .char_width = char_width,
            .char_height = char_height,
            .char_to_rect = char_to_rect,
            .white_rect = white_rect,
            .right_down_arrow_rect = right_down_arrow_rect,
            .down_right_arrow_rect = down_right_arrow_rect,
        };
    }

    pub fn deinit(self: *Atlas) void {
        self.allocator.free(self.char_to_rect);
        self.allocator.free(self.texture);
        c.TTF_CloseFont(self.font);
        c.TTF_Quit();
    }
};
