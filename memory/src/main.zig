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
const max_vertex_memory = 512 * 1024;
const max_element_memory = 128 * 1024;

var win: *SDL_Window = undefined;
var running: bool = true;

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

    while (running) main_loop(ctx);

    nk_sdl_shutdown();
    SDL_GL_DeleteContext(glContext);
    SDL_DestroyWindow(win);
    SDL_Quit();

    warn("fin\n", .{});
}

fn main_loop(ctx: *nk_context) void {

    // input
    var evt: SDL_Event = undefined;
    nk_input_begin(ctx);
    while (nk_true == SDL_PollEvent(&evt)) {
        if (evt.type == SDL_QUIT) {
            running = false;
        }
        _ = nk_sdl_handle_event(&evt);
    }
    nk_input_end(ctx);

    // gui
    if (nk_true == nk_begin(ctx, "Demo", nk_rect(50, 50, 200, 200),
                 NK_WINDOW_BORDER|NK_WINDOW_MOVABLE|NK_WINDOW_SCALABLE|
                     NK_WINDOW_CLOSABLE|NK_WINDOW_MINIMIZABLE|NK_WINDOW_TITLE))
        {
            nk_menubar_begin(ctx);
            nk_layout_row_begin(ctx, .NK_STATIC, 25, 2);
            nk_layout_row_push(ctx, 45);
            if (nk_true == nk_menu_begin_label(ctx, "FILE", NK_TEXT_LEFT, nk_vec2(120, 200))) {
                nk_layout_row_dynamic(ctx, 30, 1);
                _ = nk_menu_item_label(ctx, "OPEN", NK_TEXT_LEFT);
                _ = nk_menu_item_label(ctx, "CLOSE", NK_TEXT_LEFT);
                nk_menu_end(ctx);
            }
            nk_layout_row_push(ctx, 45);
            if (nk_true == nk_menu_begin_label(ctx, "EDIT", NK_TEXT_LEFT, nk_vec2(120, 200))) {
                nk_layout_row_dynamic(ctx, 30, 1);
                _ = nk_menu_item_label(ctx, "COPY", NK_TEXT_LEFT);
                _ = nk_menu_item_label(ctx, "CUT", NK_TEXT_LEFT);
                _ = nk_menu_item_label(ctx, "PASTE", NK_TEXT_LEFT);
                nk_menu_end(ctx);
            }
            nk_layout_row_end(ctx);
            nk_menubar_end(ctx);

            // enum {EASY, HARD};
            var op: u8 = 0;
            var property: c_int = 20;
            nk_layout_row_static(ctx, 30, 80, 1);
            if (nk_true == nk_button_label(ctx, "button"))
                _ = fprintf(stdout, "button pressed\n");
            nk_layout_row_dynamic(ctx, 30, 2);
            if (nk_true == nk_option_label(ctx, "easy", nk_bool(op == 0))) op = 0;
            if (nk_true == nk_option_label(ctx, "hard", nk_bool(op == 1))) op = 1;
            nk_layout_row_dynamic(ctx, 25, 1);
            nk_property_int(ctx, "Compression:", 0, &property, 100, 10, 1);
    }
    nk_end(ctx);

    // draw
    var bg: [4]f32 = .{0, 0, 0, 0};
    // BUG
    // nk_rgb returns different values on each call
    // suspect bad abi
    // adding dummy bytes fixes it
    // https://github.com/ziglang/zig/issues/1481
    nk_color_fv(&bg, nk_rgb(28,48,62));
    var win_width: c_int = undefined;
    var win_height: c_int = undefined;
    SDL_GetWindowSize(win, &win_width, &win_height);
    glViewport(0, 0, win_width, win_height);
    glClear(GL_COLOR_BUFFER_BIT);
    glClearColor(bg[0], bg[1], bg[2], bg[3]);
    nk_sdl_render(.NK_ANTI_ALIASING_ON, max_vertex_memory, max_element_memory);
    SDL_GL_SwapWindow(win);

}

fn nk_bool(b: bool) c_int {
    return if (b) nk_true else nk_false;
}
