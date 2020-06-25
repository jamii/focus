const focus = @import("../focus.zig");
usingnamespace focus.common;
usingnamespace focus.common.c;
const atlas = focus.atlas;

pub const Coord = u16;

pub const Rect = struct {
    x: Coord,
    y: Coord,
    w: Coord,
    h: Coord,

    pub fn shrink(self: *const Rect, margin: Coord) Rect {
        assert(self.w >= 2 * margin);
        assert(self.h >= 2 * margin);
        return Rect{.x=self.x+margin, .y=self.y+margin, .w=self.w-(2*margin), .h=self.h-(2*margin)};
    }

    pub fn splitRight(self: *Rect, w: Coord, margin: Coord) Rect {
        assert(self.w >= w);
        const split = Rect{.x=self.x+self.w-w, .y=self.y, .w=w, .h=self.h};
        self.w -= w + margin;
        return split;
    }

    pub fn splitBottom(self: *Rect, h: Coord, margin: Coord) Rect {
        assert(self.h >= h);
        const split = Rect{.x=self.x, .y=self.y+self.h-h, .w=self.w, .h=h};
        self.h -= h + margin;
        return split;
    }
};

pub const Vec2 = struct {
    x: Coord,
    y: Coord,
};

pub const Color = packed struct {
    r: u8,
    g: u8,
    b: u8,
    a: u8,
};

const Vec2f = packed struct {
    x: f32,
    y: f32,
};

fn Tri(comptime t: type) type {
    // TODO which direction?
    return packed struct {
        a: t,
        b: t,
        c: t,
    };
}

fn Quad(comptime t: type) type {
    return packed struct {
        tl: t,
        tr: t,
        bl: t,
        br: t,
    };
}

const buffer_size = 2 ^ 14;
var texture_buffer = std.mem.zeroes([buffer_size]Quad(Vec2f));
var vertex_buffer = std.mem.zeroes([buffer_size]Quad(Vec2f));
var color_buffer = std.mem.zeroes([buffer_size]Quad(Color));
var index_buffer = std.mem.zeroes([buffer_size][2]Tri(u32));

pub const scale = 1;

pub const screen_width = 720;
pub const screen_height = 1440;

var buffer_ix: usize = 0;

var window: *c.SDL_Window = undefined;

pub fn init() void {
    // init SDL
    _ = SDL_Init(SDL_INIT_EVERYTHING);
    const window_o = SDL_CreateWindow("focus", SDL_WINDOWPOS_UNDEFINED, SDL_WINDOWPOS_UNDEFINED, @as(c_int, @divTrunc(screen_width, scale)), @as(c_int, @divTrunc(screen_height, scale)), SDL_WINDOW_OPENGL | SDL_WINDOW_BORDERLESS | SDL_WINDOW_ALLOW_HIGHDPI);
    if (window_o == null) {
        warn("SDL_CreateWindow failed: {s}", .{SDL_GetError()});
        std.os.exit(1);
    }
    window = window_o.?;
    if (std.Target.current.cpu.arch == .aarch64) {
        _ = SDL_SetWindowFullscreen(window, SDL_WINDOW_FULLSCREEN);
    }
    _ = SDL_GL_CreateContext(window);

    // init gl
    glEnable(GL_BLEND);
    glBlendFunc(GL_SRC_ALPHA, GL_ONE_MINUS_SRC_ALPHA);
    glDisable(GL_CULL_FACE);
    glDisable(GL_DEPTH_TEST);
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

    // sync with monitor - causes input lag
    _ = SDL_GL_SetSwapInterval(1);
}

fn flush() void {
    if (buffer_ix == 0) {
        return;
    }

    glViewport(0, 0, @divTrunc(screen_width, scale), @divTrunc(screen_height, scale));
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
    if (buffer_ix == buffer_size) {
        flush();
    }

    const tx = @intToFloat(f32, src.x) / @intToFloat(f32, atlas.width);
    const ty = @intToFloat(f32, src.y) / @intToFloat(f32, atlas.height);
    const tw = @intToFloat(f32, src.w) / @intToFloat(f32, atlas.width);
    const th = @intToFloat(f32, src.h) / @intToFloat(f32, atlas.height);
    texture_buffer[buffer_ix] = .{
        .tl = .{ .x = tx, .y = ty },
        .tr = .{ .x = tx + tw, .y = ty },
        .bl = .{ .x = tx, .y = ty + th },
        .br = .{ .x = tx + tw, .y = ty + th },
    };

    const vx = @intToFloat(f32, dst.x);
    const vy = @intToFloat(f32, dst.y);
    const vw = @intToFloat(f32, dst.w);
    const vh = @intToFloat(f32, dst.h);
    vertex_buffer[buffer_ix] = .{
        .tl = .{ .x = vx, .y = vy },
        .tr = .{ .x = vx + vw, .y = vy },
        .bl = .{ .x = vx, .y = vy + vh },
        .br = .{ .x = vx + vw, .y = vy + vh },
    };

    color_buffer[buffer_ix] = .{
        .tl = color,
        .tr = color,
        .bl = color,
        .br = color,
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
        },
    };

    buffer_ix += 1;
}

pub fn rect(dst: Rect, color: Color) void {
    quad(dst, atlas.white, color);
}

// TODO going to need to be able to clip text
pub fn text(chars: []const u8, pos: Vec2, color: Color) void {
    var dst: Rect = .{ .x = pos.x, .y = pos.y, .w = 0, .h = 0 };
    for (chars) |char| {
        const src = atlas.chars[min(char, 127)];
        dst.w = src.w * atlas.scale;
        dst.h = src.h * atlas.scale;
        quad(dst, src, color);
        dst.x += @intCast(u16, atlas.max_char_width);
    }
}

pub fn clear(color: Color) void {
    flush();
    glClearColor(@intToFloat(f32, color.r) / 255., @intToFloat(f32, color.g) / 255., @intToFloat(f32, color.b) / 255., @intToFloat(f32, color.a) / 255.);
    glClear(GL_COLOR_BUFFER_BIT);
}

pub fn swap() void {
    flush();
    SDL_GL_SwapWindow(window);
}
