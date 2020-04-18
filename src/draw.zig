usingnamespace @import("common.zig");

pub const Vec2f = packed struct {
    x: f32,
    y: f32,
};

pub fn Tri(comptime t: type) type {
    // TODO which direction?
    return packed struct {
        a: t,
        b: t,
        c: t,
    };
}

pub fn Quad(comptime t: type) type {
    return packed struct {
        tl: t,
        tr: t,
        bl: t,
        br: t,
    };
}

const buffer_size = 2^14;
var texture_buffer = zero([buffer_size]Quad(Vec2f));
var vertex_buffer = zero([buffer_size]Quad(Vec2f));
var color_buffer = zero([buffer_size]Quad(Color));
var index_buffer = zero([buffer_size][2]Tri(u32));

pub const screen_width = @divTrunc(720, 2);
pub const screen_height = @divTrunc(1440, 2);

var buffer_ix: usize = 0;

var window: *SDL_Window = undefined;

pub fn init() void {
    // init SDL
    _ = SDL_Init(SDL_INIT_EVERYTHING);
    window = SDL_CreateWindow(
        "focus",
        SDL_WINDOWPOS_UNDEFINED,
        SDL_WINDOWPOS_UNDEFINED,
        @as(c_int, screen_width),
        @as(c_int, screen_height),
        SDL_WINDOW_OPENGL | SDL_WINDOW_BORDERLESS | SDL_WINDOW_ALLOW_HIGHDPI // | SDL_WINDOW_RESIZABLE
    ).?;
    _ = SDL_GL_CreateContext(window);

    // init gl
    glEnable(GL_BLEND);
    glBlendFunc(GL_SRC_ALPHA, GL_ONE_MINUS_SRC_ALPHA);
    glDisable(GL_CULL_FACE);
    glDisable(GL_DEPTH_TEST);
    glEnable(GL_SCISSOR_TEST);
    glEnable(GL_TEXTURE_2D);
    glEnableClientState(GL_VERTEX_ARRAY);
    glEnableClientState(GL_TEXTURE_COORD_ARRAY);
    glEnableClientState(GL_COLOR_ARRAY);

    // init texture
    var id: u32 = undefined;
    glGenTextures(1, &id);
    glBindTexture(GL_TEXTURE_2D, id);
    glTexImage2D(GL_TEXTURE_2D, 0, GL_ALPHA, atlas.width, atlas.height, 0, GL_ALPHA, GL_UNSIGNED_BYTE, &atlas.texture);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
    assert(glGetError() == 0);

    // sync swap to monitor refresh rate - blocks main loop on access to gl
    _ = SDL_GL_SetSwapInterval(1);
}

fn flush() void {
    if (buffer_ix == 0) { return; }

    glViewport(0, 0, screen_width, screen_height);
    glMatrixMode(GL_PROJECTION);
    glPushMatrix();
    glLoadIdentity();
    glOrtho(0.0, @intToFloat(f32, screen_width), @intToFloat(f32, screen_height), 0.0, -1.0, 1.0);
    glMatrixMode(GL_MODELVIEW);
    glPushMatrix();
    glLoadIdentity();

    glTexCoordPointer(2, GL_FLOAT, 0, &texture_buffer);
    glVertexPointer(2, GL_FLOAT, 0, &vertex_buffer);
    glColorPointer(4, GL_UNSIGNED_BYTE, 0, &color_buffer);
    glDrawElements(GL_TRIANGLES, @intCast(c_int, buffer_ix) * 6, GL_UNSIGNED_INT, &index_buffer);

    glMatrixMode(GL_MODELVIEW);
    glPopMatrix();
    glMatrixMode(GL_PROJECTION);
    glPopMatrix();

    buffer_ix = 0;
}

fn quad(dst: Rect, src: Rect, color: Color) void {
    if (buffer_ix == buffer_size) { flush(); }

    const tx = @intToFloat(f32, src.x) / @intToFloat(f32, atlas.width);
    const ty = @intToFloat(f32, src.y) / @intToFloat(f32, atlas.height);
    const tw = @intToFloat(f32, src.w) / @intToFloat(f32, atlas.width);
    const th = @intToFloat(f32, src.h) / @intToFloat(f32, atlas.height);
    texture_buffer[buffer_ix] = .{
        .tl = .{.x=tx, .y=ty},
        .tr = .{.x=tx+tw, .y=ty},
        .bl = .{.x=tx, .y=ty+th},
        .br = .{.x=tx+tw, .y=ty+th},
    };

    const vx = @intToFloat(f32, dst.x);
    const vy = @intToFloat(f32, dst.y);
    const vw = @intToFloat(f32, dst.w);
    const vh = @intToFloat(f32, dst.h);
    vertex_buffer[buffer_ix] = .{
        .tl = .{.x=vx, .y=vy},
        .tr = .{.x=vx+vw, .y=vy},
        .bl = .{.x=vx, .y=vy+vh},
        .br = .{.x=vx+vw, .y=vy+vh},
    };

    color_buffer[buffer_ix] = .{
        .tl = color,
        .tr = color,
        .bl = color,
        .br = color
    };

    const vertex_ix = @intCast(u32, buffer_ix * 4);
    index_buffer[buffer_ix] = .{
        .{
            .a = vertex_ix + 0,
            .b = vertex_ix + 1,
            .c = vertex_ix + 2,
        },
        .{
            .a = vertex_ix + 2,
            .b = vertex_ix + 3,
            .c = vertex_ix + 1,
        }
    };

    buffer_ix += 1;
}

pub fn rect(dst: Rect, color: Color) void {
    quad(dst, atlas.white, color);
}

pub fn text(str: []const u8, pos: Vec2, color: Color) void {
    var dst: Rect = .{ .x = pos.x, .y = pos.y, .w = 0, .h = 0 };
    for (str) |p| {
        const chr = std.math.min(p, 127);
        const src = atlas.chars[chr];
        dst.w = src.w * 2;
        dst.h = src.h * 2;
        quad(dst, src, color);
        dst.x += dst.w;
    }
}

pub fn set_clip(clip: Rect) void {
    flush();
    glScissor(
        @intCast(c_int, clip.x),
        @intCast(c_int, screen_height - (clip.y + clip.h)),
        @intCast(c_int, clip.w),
        @intCast(c_int, clip.h),
    );
}

pub fn clear(color: Color) void {
    flush();
    glClearColor(
        @intToFloat(f32, color.r) / 255.,
        @intToFloat(f32, color.g) / 255.,
        @intToFloat(f32, color.b) / 255.,
        @intToFloat(f32, color.a) / 255.
    );
    glClear(GL_COLOR_BUFFER_BIT);
}

pub fn swap() void {
    flush();
    SDL_GL_SwapWindow(window);
}
