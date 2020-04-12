const std = @import("std");
const warn = std.debug.warn;
const assert = std.debug.assert;

const c_str = [*c]u8;
const c_const_str = [*c]const u8;
pub usingnamespace @cImport({
    @cInclude("SDL2/SDL.h");
    @cInclude("SDL2/SDL_opengles2.h");
    @cInclude("nuklear.h");
    @cInclude("nk_main.h");
    @cInclude("nuklear_sdl_gles2.h");
});

const window_width = 800;
const window_height = 600;

var win: *SDL_Window = undefined;
var running: bool = false;

pub fn main() anyerror!void {
    var ctx: *nk_context = undefined;
    var glContext: SDL_GLContext = undefined;

    // SDL setup
    _ = SDL_SetHint(SDL_HINT_VIDEO_HIGHDPI_DISABLED, "0");
    _ = SDL_GL_SetAttribute (.SDL_GL_CONTEXT_FLAGS, SDL_GL_CONTEXT_FORWARD_COMPATIBLE_FLAG);
    _ = SDL_GL_SetAttribute (.SDL_GL_CONTEXT_PROFILE_MASK, SDL_GL_CONTEXT_PROFILE_CORE);
    _ = SDL_GL_SetAttribute(.SDL_GL_CONTEXT_MAJOR_VERSION, 3);
    _ = SDL_GL_SetAttribute(.SDL_GL_CONTEXT_MINOR_VERSION, 3);
    _ = SDL_GL_SetAttribute(.SDL_GL_DOUBLEBUFFER, 1);
    win = SDL_CreateWindow("Demo",
                           SDL_WINDOWPOS_CENTERED, SDL_WINDOWPOS_CENTERED,
                           window_width, window_height, SDL_WINDOW_OPENGL|SDL_WINDOW_SHOWN|SDL_WINDOW_ALLOW_HIGHDPI).?;
    glContext = SDL_GL_CreateContext(win);

    // OpenGL setup
    glViewport(0, 0, window_width, window_height);
    ctx = nk_sdl_init(win);
    var atlas: ?*nk_font_atlas = undefined;
    nk_sdl_font_stash_begin(&atlas);
    nk_sdl_font_stash_end();

    while (running) MainLoop(ctx);

    nk_sdl_shutdown();
    SDL_GL_DeleteContext(glContext);
    SDL_DestroyWindow(win);
    SDL_Quit();

    warn("fin", .{});
}
