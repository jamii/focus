const focus = @import("../focus.zig");
usingnamespace focus.common;
pub const draw = focus.draw;

const fira_code = @embedFile("../../fonts/Fira_Code_v5.2/ttf/FiraCode-Regular.ttf");

pub const Atlas = struct {
    allocator: *Allocator,
    font: *c.TTF_Font,
    texture: []draw.Color,
    texture_dims: draw.Vec2,
    char_width: u16,
    char_height: u16,
    char_to_rect: []draw.Rect,
    white_rect: draw.Rect,

    pub const point_size = 10;
    
    pub fn init(allocator: *Allocator) ! Atlas {

        // init SDL2_ttf
        if (c.TTF_Init() != 0)
            panic("TTF_Init failed: {s}", .{c.TTF_GetError()});
        errdefer(c.TTF_Quit());

        // load font
        var reader = c.SDL_RWFromConstMem(fira_code, @intCast(c_int, fira_code.len))
            orelse panic("SDL_RWFromMem failed: {s}", .{c.SDL_GetError()});
        const font = c.TTF_OpenFontRW(
            reader,
            1, // automatically close reader
            16, // point_size,
        ) orelse panic("Font load failed: {s}", .{c.TTF_GetError()});
        errdefer c.TTF_CloseFont(font);

        // render all ascii chars
        var text = try allocator.allocSentinel(u8, 128, 0);
        defer allocator.free(text);
        text[0] = ' '; // going to overwrite this with pure white in final texture
        {
            var char: usize = 1;
            while (char <= 128) : (char += 1) {
                text[char] = @intCast(u8, char);
            }
        }
        const surface = c.TTF_RenderUTF8_Blended(font, text, c.SDL_Color{.r=255, .g=255, .b=255, .a=255})
            orelse panic("Atlas render failed: {s}", .{c.TTF_GetError()});
        defer c.SDL_FreeSurface(surface);

        // turn sdl surface into gl texture
        assert(surface.*.format.*.format == c.SDL_PIXELFORMAT_ARGB8888);
        const Pixel = packed struct {
            a: u8,
            r: u8,
            g: u8,
            b: u8,
        };
        const pixels = @ptrCast([*]Pixel, surface.*.pixels);
        const surface_len = @intCast(usize, surface.*.w * surface.*.h);
        var texture = try allocator.alloc(draw.Color, surface_len);
        errdefer allocator.free(texture);
        {
            var i: usize = 0;
            while (i < surface_len) : (i += 1) {
                const pixel = pixels[i];
                texture[i] = draw.Color{.r=pixel.r, .g=pixel.g, .b=pixel.b, .a=pixel.a};
            }
        }

        // make a white pixel
        texture[0] = draw.Color{.r=255, .g=255, .b=255, .a=255};
        const white_rect = draw.Rect{.x=0, .y=0, .w=1, .h=1};

        // calculate char sizes
        // assume monospaced font
        const char_width = @intCast(u16, @divTrunc(surface.*.w, 127));
        const char_height = @intCast(u16, surface.*.h);

        // calculate location of each char
        var char_to_rect = try allocator.alloc(draw.Rect, 128);
        errdefer allocator.free(char_to_rect);
        char_to_rect[0] = .{.x=0, .y=0, .w=0, .h=0};
        {
            var char: usize = 1;
            while (char < 128) : (char += 1) {
                char_to_rect[char] = .{
                    .x = @intCast(u16, char-1) * char_width,
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
                .x = @intCast(u16, surface.*.w),
                .y = @intCast(u16, surface.*.h),
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
