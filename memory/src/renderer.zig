usingnamespace @import("common.zig");
usingnamespace @import("atlas.zig");

const buffer_size = 16384;

var tex_buf: [buffer_size * 8]GLfloat = @splat(buffer_size * 8, @as(GLfloat, 0));
var vert_buf: [buffer_size * 8]GLfloat = @splat(buffer_size * 8, @as(GLfloat, 0));
var color_buf: [buffer_size * 16]GLubyte = @splat(buffer_size * 16, @as(GLubyte, 0));
var index_buf: [buffer_size * 6]GLuint = @splat(buffer_size * 6, @as(GLuint, 0));

const width = 800;
const height = 600;
var buf_idx: GLuint = 0;

var window: *SDL_Window = undefined;

pub fn r_init() void {
    // init SDL window
    window = SDL_CreateWindow(
        "focus", SDL_WINDOWPOS_UNDEFINED, SDL_WINDOWPOS_UNDEFINED,
        @as(c_int, width), @as(c_int, height), SDL_WINDOW_OPENGL | SDL_WINDOW_BORDERLESS | SDL_WINDOW_ALLOW_HIGHDPI).?; // | SDL_WINDOW_RESIZABLE
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
    var id: GLuint = undefined;
    glGenTextures(1, &id);
    glBindTexture(GL_TEXTURE_2D, id);
    glTexImage2D(GL_TEXTURE_2D, 0, GL_ALPHA, ATLAS_WIDTH, ATLAS_HEIGHT, 0,
                 GL_ALPHA, GL_UNSIGNED_BYTE, &atlas_texture);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
    assert(glGetError() == 0);
}

fn flush() void {
    if (buf_idx == 0) { return; }

    glViewport(0, 0, width, height);
    glMatrixMode(GL_PROJECTION);
    glPushMatrix();
    glLoadIdentity();
    glOrtho(0.0, @intToFloat(GLfloat, width), @intToFloat(GLfloat, height), 0.0, -1.0, 1.0);
    glMatrixMode(GL_MODELVIEW);
    glPushMatrix();
    glLoadIdentity();

    glTexCoordPointer(2, GL_FLOAT, 0, &tex_buf);
    glVertexPointer(2, GL_FLOAT, 0, &vert_buf);
    glColorPointer(4, GL_UNSIGNED_BYTE, 0, &color_buf);
    glDrawElements(GL_TRIANGLES, @intCast(c_int, buf_idx) * 6, GL_UNSIGNED_INT, &index_buf);

    glMatrixMode(GL_MODELVIEW);
    glPopMatrix();
    glMatrixMode(GL_PROJECTION);
    glPopMatrix();

    buf_idx = 0;
}

fn push_quad(dst: mu_Rect, src: mu_Rect, color: mu_Color) void {
    if (buf_idx == buffer_size) { flush(); }

    var texvert_idx: usize = buf_idx * 8;
    var color_idx: usize = buf_idx * 16;
    var element_idx: c_uint = buf_idx * 4;
    var index_idx: usize = buf_idx * 6;
    buf_idx += 1;

    // update texture buffer
    const x: GLfloat = @intToFloat(GLfloat, src.x) / @intToFloat(GLfloat, ATLAS_WIDTH);
    const y: GLfloat = @intToFloat(GLfloat, src.y) / @intToFloat(GLfloat, ATLAS_HEIGHT);
    const w: GLfloat = @intToFloat(GLfloat, src.w) / @intToFloat(GLfloat, ATLAS_WIDTH);
    const h: GLfloat = @intToFloat(GLfloat, src.h) / @intToFloat(GLfloat, ATLAS_HEIGHT);
    tex_buf[texvert_idx + 0] = x;
    tex_buf[texvert_idx + 1] = y;
    tex_buf[texvert_idx + 2] = x + w;
    tex_buf[texvert_idx + 3] = y;
    tex_buf[texvert_idx + 4] = x;
    tex_buf[texvert_idx + 5] = y + h;
    tex_buf[texvert_idx + 6] = x + w;
    tex_buf[texvert_idx + 7] = y + h;

    // update vertex buffer
    vert_buf[texvert_idx + 0] = @intToFloat(GLfloat, dst.x);
    vert_buf[texvert_idx + 1] = @intToFloat(GLfloat, dst.y);
    vert_buf[texvert_idx + 2] = @intToFloat(GLfloat, dst.x + dst.w);
    vert_buf[texvert_idx + 3] = @intToFloat(GLfloat, dst.y);
    vert_buf[texvert_idx + 4] = @intToFloat(GLfloat, dst.x);
    vert_buf[texvert_idx + 5] = @intToFloat(GLfloat, dst.y + dst.h);
    vert_buf[texvert_idx + 6] = @intToFloat(GLfloat, dst.x + dst.w);
    vert_buf[texvert_idx + 7] = @intToFloat(GLfloat, dst.y + dst.h);

    // update color buffer
    _ = @memcpy(@ptrCast([*]u8, color_buf[color_idx + 0..]), @ptrCast([*]const u8, &color), 4);
    _ = @memcpy(@ptrCast([*]u8, color_buf[color_idx + 4..]), @ptrCast([*]const u8, &color), 4);
    _ = @memcpy(@ptrCast([*]u8, color_buf[color_idx + 8..]), @ptrCast([*]const u8, &color), 4);
    _ = @memcpy(@ptrCast([*]u8, color_buf[color_idx + 12..]), @ptrCast([*]const u8, &color), 4);

    // update index buffer
    index_buf[index_idx + 0] = element_idx + 0;
    index_buf[index_idx + 1] = element_idx + 1;
    index_buf[index_idx + 2] = element_idx + 2;
    index_buf[index_idx + 3] = element_idx + 2;
    index_buf[index_idx + 4] = element_idx + 3;
    index_buf[index_idx + 5] = element_idx + 1;
}

pub fn r_draw_rect(rect: mu_Rect, color: mu_Color) void {
    push_quad(rect, atlas[ATLAS_WHITE], color);
}

pub fn r_draw_text(text: []const u8, pos: mu_Vec2, color: mu_Color) void {
    var dst: mu_Rect = .{ .x = pos.x, .y = pos.y, .w = 0, .h = 0, .dummy = undefined };
    for (text) |p| {
        const chr: u8 = std.math.min(p, 127);
        const src: mu_Rect = atlas[@as(usize, ATLAS_FONT) + @as(usize, chr)];
        dst.w = src.w;
        dst.h = src.h;
        push_quad(dst, src, color);
        dst.x += dst.w;
    }
}

pub fn r_draw_icon(id: c_int, rect: mu_Rect, color: mu_Color) void {
    const src: mu_Rect = atlas[@intCast(usize, id)];
    const x: c_int = rect.x + @divTrunc((rect.w - src.w), 2);
    const y: c_int = rect.y + @divTrunc((rect.h - src.h), 2);
    push_quad(mu_rect(x, y, src.w, src.h), src, color);
}

pub fn r_get_text_width(text: [*c]const u8, len: c_int) c_int {
    var res: c_int = 0;
    var p = text;
    while (p.* != 0) : (p += 1) {
        const chr: u8 = std.math.min(p.*, 127);
        res += atlas[@as(usize, ATLAS_FONT) + @as(usize, chr)].w;
    }
    return res;
}

pub fn r_get_text_height() c_int {
    return 18;
}

pub fn r_set_clip_rect(rect: mu_Rect) void {
    flush();
    glScissor(rect.x, height - (rect.y + rect.h), rect.w, rect.h);
}

pub fn r_clear(clr: mu_Color) void {
    flush();
    glClearColor(@intToFloat(GLfloat, clr.r) / 255., @intToFloat(GLfloat, clr.g) / 255., @intToFloat(GLfloat, clr.b) / 255., @intToFloat(GLfloat, clr.a) / 255.);
    glClear(GL_COLOR_BUFFER_BIT);
}

pub fn r_present() void {
    flush();
    SDL_GL_SwapWindow(window);
}
