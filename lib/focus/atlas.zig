const focus = @import("../focus.zig");
usingnamespace focus.common;
pub const ui = focus.ui;

const fira_code = @embedFile("../../fonts/Fira_Code_v5.2/ttf/FiraCode-Regular.ttf");

pub const Atlas = struct {
    allocator: *Allocator,
    font: *c.TTF_Font,
    texture: []Color,
    texture_dims: Vec2,
    char_width: Coord,
    char_height: Coord,
    char_to_rect: []Rect,
    white_rect: Rect,

    pub const point_size = 10;
    
    pub fn init(allocator: *Allocator) ! Atlas {

        // init SDL2_ttf
        if (c.TTF_Init() != 0)
            panic("TTF_Init failed: {s}", .{c.TTF_GetError()});
        errdefer(c.TTF_Quit());

        // load font
        var reader = c.SDL_RWFromConstMem(fira_code, @intCast(c_int, fira_code.len))
            orelse panic("Font reader failed: {s}", .{c.SDL_GetError()});
        const font = c.TTF_OpenFontRW(
            reader,
            1, // automatically close reader
            16, // point_size,
        ) orelse panic("Font load failed: {s}", .{c.TTF_GetError()});
        errdefer c.TTF_CloseFont(font);

        // render all ascii chars
        var text = try allocator.allocSentinel(u8, 128, 0);
        defer allocator.free(text);
        text[0] = ' '; // going to overwrite this with a white block in final texture
        {
            var char: usize = 1;
            while (char < text.len) : (char += 1) {
                text[char] = @intCast(u8, char);
            }
        }
        const surface = c.TTF_RenderUTF8_Blended(font, text, c.SDL_Color{.r=255, .g=255, .b=255, .a=255})
            orelse panic("Atlas render failed: {s}", .{c.TTF_GetError()});
        defer c.SDL_FreeSurface(surface);

        // copy the texture
        assert(surface.*.format.*.format == c.SDL_PIXELFORMAT_ARGB8888);
        const texture = try std.mem.dupe(allocator, Color, @ptrCast([*]Color, surface.*.pixels)[0..@intCast(usize, surface.*.w * surface.*.h)]);

        // make a white pixel
        texture[0] = Color{.r=255, .g=255, .b=255, .a=255};
        const white_rect = Rect{.x=0, .y=0, .w=1, .h=1};

        // calculate char sizes
        // assume monospaced font
        const char_width = @intCast(Coord, @divTrunc(@intCast(usize, surface.*.w), text.len));
        const char_height = @intCast(Coord, surface.*.h);

        // calculate location of each char
        var char_to_rect = try allocator.alloc(Rect, text.len);
        errdefer allocator.free(char_to_rect);
        char_to_rect[0] = .{.x=0, .y=0, .w=0, .h=0};
        {
            var char: usize = 1;
            while (char < text.len) : (char += 1) {
                char_to_rect[char] = .{
                    .x = @intCast(Coord, char) * char_width,
                    .y = 0,
                    .w = char_width,
                    .h = char_height,
                };
            }
        }

        return Atlas{
            .allocator = allocator,
            .font = font,
            .texture = texture,
            .texture_dims = .{
                .x = @intCast(Coord, surface.*.w),
                .y = @intCast(Coord, surface.*.h),
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
        c.TTF_CloseFont(self.font);
        c.TTF_Quit();
    }    
};
