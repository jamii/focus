const std = @import("std");
const warn = std.debug.warn;
const assert = std.debug.assert;


const char = u8;
const c_str = [*c]char;
const c_const_str = [*c]const char;
pub usingnamespace @cImport({
    @cInclude("SDL2/SDL.h");
    @cInclude("SDL2/SDL_opengles2.h");
    // these have to be the same as in nuklear.c
    // apart from NK_IMPLEMENTATION
    @cDefine("NK_INCLUDE_FIXED_TYPES", {});
    @cDefine("NK_INCLUDE_STANDARD_IO", {});
    @cDefine("NK_INCLUDE_STANDARD_VARARGS", {});
    @cDefine("NK_INCLUDE_DEFAULT_ALLOCATOR", {});
    @cDefine("NK_INCLUDE_VERTEX_BUFFER_OUTPUT", {});
    @cDefine("NK_INCLUDE_FONT_BAKING", {});
    @cDefine("NK_INCLUDE_DEFAULT_FONT", {});
    @cInclude("nuklear.h");
});
// usingnamespace @import("nk_sdl_gles2.zig");


const window_width = 800;
const window_height = 600;
const max_vertex_memory = 512 * 1024;
const max_element_memory = 128 * 1024;

var running: bool = true;
var sdl: nk_sdl = undefined; // TODO should be 0?

pub fn main() anyerror!void {
    var win: *SDL_Window = undefined;
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
    var atlas: *nk_font_atlas = undefined;
    nk_sdl_font_stash_begin(&atlas);
    nk_sdl_font_stash_end();
    set_style(ctx);

    while (running) main_loop(win, ctx);

    nk_sdl_shutdown();
    SDL_GL_DeleteContext(glContext);
    SDL_DestroyWindow(win);
    SDL_Quit();

    warn("fin\n", .{});
}

// enum {EASY, HARD};
var op: u8 = 0;
var property: c_int = 20;

fn main_loop(win: *SDL_Window, ctx: *nk_context) void {

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
    if (nk_true == nk_begin(ctx, "Demo", nk_rect(0, 0, window_width, window_height), 0))
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
    nk_sdl_render(win, .NK_ANTI_ALIASING_ON, max_vertex_memory, max_element_memory);
    SDL_GL_SwapWindow(win);

}

fn nk_bool(b: bool) c_int {
    return if (b) nk_true else nk_false;
}

const nk_sdl_device = struct {
    cmds: nk_buffer,
    @"null": nk_draw_null_texture,
    vbo: GLuint,
    ebo: GLuint,
    prog: GLuint,
    vert_shdr: GLuint,
    frag_shdr: GLuint,
    attrib_pos: GLint,
    attrib_uv: GLint,
    attrib_col: GLint,
    uniform_tex: GLint,
    uniform_proj: GLint,
    font_tex: GLuint,
    vs: GLsizei,
    vp: usize,
    vt: usize,
    vc: usize,
};

const nk_sdl_vertex = struct {
    position: [2]GLfloat,
    uv: [2]GLfloat,
    col: [4]nk_byte,
};

const nk_sdl = struct {
    win: *SDL_Window,
    ogl: nk_sdl_device,
    ctx: nk_context,
    atlas: nk_font_atlas,
};

fn nk_sdl_device_create() void
{
    var status: GLint = undefined;
    const vertex_shader: [*c]const GLchar =
        \\ #version 100
        \\ uniform mat4 ProjMtx;
        \\ attribute vec2 Position;
        \\ attribute vec2 TexCoord;
        \\ attribute vec4 Color;
        \\ varying vec2 Frag_UV;
        \\ varying vec4 Frag_Color;
        \\ void main() {
        \\    Frag_UV = TexCoord;
        \\    Frag_Color = Color;
        \\    gl_Position = ProjMtx * vec4(Position.xy, 0, 1);
        \\ }
    ;
    const fragment_shader: [*c]const GLchar =
        \\ #version 100
        \\ precision mediump float;
        \\ uniform sampler2D Texture;
        \\ varying vec2 Frag_UV;
        \\ varying vec4 Frag_Color;
        \\ void main(){
        \\    gl_FragColor = Frag_Color * texture2D(Texture, Frag_UV);
        \\ }
    ;
    const dev: *nk_sdl_device = &sdl.ogl;

    nk_buffer_init_default(&dev.cmds);
    dev.prog = glCreateProgram();
    dev.vert_shdr = glCreateShader(GL_VERTEX_SHADER);
    dev.frag_shdr = glCreateShader(GL_FRAGMENT_SHADER);
    glShaderSource(dev.vert_shdr, 1, &vertex_shader, 0);
    glShaderSource(dev.frag_shdr, 1, &fragment_shader, 0);
    glCompileShader(dev.vert_shdr);
    glCompileShader(dev.frag_shdr);
    glGetShaderiv(dev.vert_shdr, GL_COMPILE_STATUS, &status);
    assert(status == GL_TRUE);
    glGetShaderiv(dev.frag_shdr, GL_COMPILE_STATUS, &status);
    assert(status == GL_TRUE);
    glAttachShader(dev.prog, dev.vert_shdr);
    glAttachShader(dev.prog, dev.frag_shdr);
    glLinkProgram(dev.prog);
    glGetProgramiv(dev.prog, GL_LINK_STATUS, &status);
    assert(status == GL_TRUE);

    dev.uniform_tex = glGetUniformLocation(dev.prog, "Texture");
    dev.uniform_proj = glGetUniformLocation(dev.prog, "ProjMtx");
    dev.attrib_pos = glGetAttribLocation(dev.prog, "Position");
    dev.attrib_uv = glGetAttribLocation(dev.prog, "TexCoord");
    dev.attrib_col = glGetAttribLocation(dev.prog, "Color");
    {
        dev.vs = @sizeOf(nk_sdl_vertex);
        dev.vp = @byteOffsetOf(nk_sdl_vertex, "position");
        dev.vt = @byteOffsetOf(nk_sdl_vertex, "uv");
        dev.vc = @byteOffsetOf(nk_sdl_vertex, "col");

        // Allocate buffers
        glGenBuffers(1, &dev.vbo);
        glGenBuffers(1, &dev.ebo);
    }
    glBindTexture(GL_TEXTURE_2D, 0);
    glBindBuffer(GL_ARRAY_BUFFER, 0);
    glBindBuffer(GL_ELEMENT_ARRAY_BUFFER, 0);
}

fn nk_sdl_device_upload_atlas(image: *const c_void, width: c_int, height: c_int) void {
    const dev: *nk_sdl_device = &sdl.ogl;
    glGenTextures(1, &dev.font_tex);
    glBindTexture(GL_TEXTURE_2D, dev.font_tex);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR);
    glTexImage2D(GL_TEXTURE_2D, 0, GL_RGBA, @intCast(GLsizei, width), @intCast(GLsizei, height), 0,
                 GL_RGBA, GL_UNSIGNED_BYTE, image);
}

fn nk_sdl_device_destroy() void {
    const dev: *nk_sdl_device = &sdl.ogl;
    glDetachShader(dev.prog, dev.vert_shdr);
    glDetachShader(dev.prog, dev.frag_shdr);
    glDeleteShader(dev.vert_shdr);
    glDeleteShader(dev.frag_shdr);
    glDeleteProgram(dev.prog);
    glDeleteTextures(1, &dev.font_tex);
    glDeleteBuffers(1, &dev.vbo);
    glDeleteBuffers(1, &dev.ebo);
    nk_buffer_free(&dev.cmds);
}

fn nk_sdl_render(win: *SDL_Window, AA: nk_anti_aliasing, max_vertex_buffer: c_int, max_element_buffer: c_int) void {
    const dev: *nk_sdl_device = &sdl.ogl;
    var width: c_int = undefined;
    var height: c_int = undefined;
    var display_width: c_int = undefined;
    var display_height: c_int = undefined;
    var scale: struct_nk_vec2 = undefined;
    var ortho: [4][4]GLfloat = .{
        .{2.0, 0.0, 0.0, 0.0},
        .{0.0,-2.0, 0.0, 0.0},
        .{0.0, 0.0,-1.0, 0.0},
        .{-1.0,1.0, 0.0, 1.0},
    };
    SDL_GetWindowSize(sdl.win, &width, &height);
    SDL_GL_GetDrawableSize(sdl.win, &display_width, &display_height);
    ortho[0][0] /= @intToFloat(GLfloat, width);
    ortho[1][1] /= @intToFloat(GLfloat, height);

    scale.x = @intToFloat(f32, display_width) / @intToFloat(f32, width);
    scale.y = @intToFloat(f32, display_height) / @intToFloat(f32, height);

    // setup global state
    glViewport(0,0,display_width,display_height);
    glEnable(GL_BLEND);
    glBlendEquation(GL_FUNC_ADD);
    glBlendFunc(GL_SRC_ALPHA, GL_ONE_MINUS_SRC_ALPHA);
    glDisable(GL_CULL_FACE);
    glDisable(GL_DEPTH_TEST);
    glEnable(GL_SCISSOR_TEST);
    glActiveTexture(GL_TEXTURE0);

    // setup program
    glUseProgram(dev.prog);
    glUniform1i(dev.uniform_tex, 0);
    glUniformMatrix4fv(dev.uniform_proj, 1, GL_FALSE, &ortho[0][0]);
    {
        // convert from command queue into draw list and draw to screen
        var vertices: *c_void = undefined;
        var elements: *c_void = undefined;
        var offset: [*c]nk_draw_index = null;

        // bind buffers
        glBindBuffer(GL_ARRAY_BUFFER, dev.vbo);
        glBindBuffer(GL_ELEMENT_ARRAY_BUFFER, dev.ebo);

        {
            // buffer setup
            glEnableVertexAttribArray(@intCast(GLuint, dev.attrib_pos));
            glEnableVertexAttribArray(@intCast(GLuint, dev.attrib_uv));
            glEnableVertexAttribArray(@intCast(GLuint, dev.attrib_col));

            glVertexAttribPointer(@intCast(GLuint, dev.attrib_pos), 2, GL_FLOAT, GL_FALSE, dev.vs, @intToPtr(?*c_void, dev.vp));
            glVertexAttribPointer(@intCast(GLuint, dev.attrib_uv), 2, GL_FLOAT, GL_FALSE, dev.vs, @intToPtr(?*c_void, dev.vt));
            glVertexAttribPointer(@intCast(GLuint, dev.attrib_col), 4, GL_UNSIGNED_BYTE, GL_TRUE, dev.vs, @intToPtr(?*c_void, dev.vc));
        }

        glBufferData(GL_ARRAY_BUFFER, max_vertex_buffer, NULL, GL_STREAM_DRAW);
        glBufferData(GL_ELEMENT_ARRAY_BUFFER, max_element_buffer, NULL, GL_STREAM_DRAW);

        // load vertices/elements directly into vertex/element buffer
        vertices = malloc(@intCast(usize, max_vertex_buffer));
        elements = malloc(@intCast(usize, max_element_buffer));
        {
            // fill convert configuration
            var config: nk_convert_config = undefined;
            const vertex_layout: [4]nk_draw_vertex_layout_element = .{
                .{
                    .attribute = .NK_VERTEX_POSITION,
                    .format = .NK_FORMAT_FLOAT,
                    .offset = @byteOffsetOf(nk_sdl_vertex, "position"),
                },
                .{
                    .attribute = .NK_VERTEX_TEXCOORD,
                    .format = .NK_FORMAT_FLOAT,
                    .offset = @byteOffsetOf(nk_sdl_vertex, "uv"),
                },
                .{
                    .attribute = .NK_VERTEX_COLOR,
                    .format = .NK_FORMAT_R8G8B8A8,
                    .offset = @byteOffsetOf(nk_sdl_vertex, "col"),
                },
                // NK_VERTEX_LAYOUT_END
                .{
                    .attribute = .NK_VERTEX_ATTRIBUTE_COUNT,
                    .format = .NK_FORMAT_COUNT,
                    .offset = 0,
                }
            };
            _ = memset(&config, 0, @sizeOf(nk_convert_config));
            config.vertex_layout = &vertex_layout;
            config.vertex_size = @sizeOf(nk_sdl_vertex);
            config.vertex_alignment = @alignOf(nk_sdl_vertex);
            config.@"null" = dev.@"null";
            config.circle_segment_count = 22;
            config.curve_segment_count = 22;
            config.arc_segment_count = 22;
            config.global_alpha = 1.0;
            config.shape_AA = AA;
            config.line_AA = AA;

            // setup buffers to load vertices and elements
            var vbuf: nk_buffer = undefined;
            var ebuf: nk_buffer = undefined;
            nk_buffer_init_fixed(&vbuf, vertices, @intCast(nk_size, max_vertex_buffer));
            nk_buffer_init_fixed(&ebuf, elements, @intCast(nk_size, max_element_buffer));
            _ = nk_convert(&sdl.ctx, &dev.cmds, &vbuf, &ebuf, &config);
        }
        glBufferSubData(GL_ARRAY_BUFFER, 0, @intCast(c_long, max_vertex_buffer), vertices);
        glBufferSubData(GL_ELEMENT_ARRAY_BUFFER, 0, @intCast(c_long, max_element_buffer), elements);
        free(vertices);
        free(elements);

        // iterate over and execute each draw command
        var cmd = nk__draw_begin(&sdl.ctx, &dev.cmds);
        while (cmd != 0) : (cmd = nk__draw_next(cmd, &dev.cmds, &sdl.ctx)) {
            if (cmd.*.elem_count == 0) continue;
            glBindTexture(GL_TEXTURE_2D, @intCast(GLuint, cmd.*.texture.id));
            glScissor(
                @floatToInt(GLint, cmd.*.clip_rect.x * scale.x),
                @floatToInt(GLint, (@intToFloat(f32, height) - (cmd.*.clip_rect.y + cmd.*.clip_rect.h)) * scale.y),
                @floatToInt(GLint, (cmd.*.clip_rect.w * scale.x)),
                @floatToInt(GLint, (cmd.*.clip_rect.h * scale.y))
            );
            glDrawElements(GL_TRIANGLES, @intCast(GLsizei, cmd.*.elem_count), GL_UNSIGNED_SHORT, offset);
            offset += cmd.*.elem_count;
        }
        nk_clear(&sdl.ctx);
    }

    glUseProgram(0);
    glBindBuffer(GL_ARRAY_BUFFER, 0);
    glBindBuffer(GL_ELEMENT_ARRAY_BUFFER, 0);

    glDisable(GL_BLEND);
    glDisable(GL_SCISSOR_TEST);
}

export fn nk_sdl_clipboard_paste(usr: nk_handle, edit: [*c]nk_text_edit) void {
    const text: [*c]const char = SDL_GetClipboardText();
    if (text != null) {
        _ = nk_textedit_paste(edit, text, nk_strlen(text));
    }
}

export fn nk_sdl_clipboard_copy(usr: nk_handle, text: [*c]const char, len: c_int) void {
    var str: [*c]char = undefined;
    if (len == 0) return;
    str = @ptrCast([*c]char, malloc(@intCast(usize, len+1)));
    if (str == null) return;
    _ = memcpy(str, text, @intCast(usize, len));
    str[@intCast(usize, len)] = 0;
    _ = SDL_SetClipboardText(str);
    free(str);
}

fn nk_sdl_init(win: *SDL_Window) *nk_context {
    sdl.win = win;
    _ = nk_init_default(&sdl.ctx, 0);
    sdl.ctx.clip.copy = nk_sdl_clipboard_copy;
    sdl.ctx.clip.paste = nk_sdl_clipboard_paste;
    sdl.ctx.clip.userdata = nk_handle_ptr(null);
    nk_sdl_device_create();
    return &sdl.ctx;
}

fn nk_sdl_font_stash_begin(atlas: **nk_font_atlas) void {
    nk_font_atlas_init_default(&sdl.atlas);
    nk_font_atlas_begin(&sdl.atlas);
    atlas.* = &sdl.atlas;
}

fn nk_sdl_font_stash_end() void {
    var w: c_int = undefined;
    var h: c_int = undefined;
    const image: *const c_void = nk_font_atlas_bake(&sdl.atlas, &w, &h, .NK_FONT_ATLAS_RGBA32).?;
    nk_sdl_device_upload_atlas(image, w, h);
    nk_font_atlas_end(&sdl.atlas, nk_handle_id(@intCast(c_int, sdl.ogl.font_tex)), &sdl.ogl.@"null");
    if (sdl.atlas.default_font != null)
        nk_style_set_font(&sdl.ctx, &sdl.atlas.default_font.*.handle);
}

fn nk_sdl_handle_event(evt: *SDL_Event) c_int {
    const ctx: *nk_context = &sdl.ctx;
    if (evt.@"type" == SDL_KEYUP or evt.@"type" == SDL_KEYDOWN) {
        // key events
        const down: c_int = if (evt.@"type" == SDL_KEYDOWN) 1 else 0;
        const state: [*c]const u8 = SDL_GetKeyboardState(0);
        const sym: SDL_Keycode = evt.key.keysym.sym;
        if (sym == SDLK_RSHIFT or sym == SDLK_LSHIFT) {
            nk_input_key(ctx, .NK_KEY_SHIFT, down);
        } else if (sym == SDLK_DELETE) {
            nk_input_key(ctx, .NK_KEY_DEL, down);
        } else if (sym == SDLK_RETURN) {
            nk_input_key(ctx, .NK_KEY_ENTER, down);
        } else if (sym == SDLK_TAB) {
            nk_input_key(ctx, .NK_KEY_TAB, down);
        } else if (sym == SDLK_BACKSPACE) {
            nk_input_key(ctx, .NK_KEY_BACKSPACE, down);
        } else if (sym == SDLK_HOME) {
            nk_input_key(ctx, .NK_KEY_TEXT_START, down);
            nk_input_key(ctx, .NK_KEY_SCROLL_START, down);
        } else if (sym == SDLK_END) {
            nk_input_key(ctx, .NK_KEY_TEXT_END, down);
            nk_input_key(ctx, .NK_KEY_SCROLL_END, down);
        } else if (sym == SDLK_PAGEDOWN) {
            nk_input_key(ctx, .NK_KEY_SCROLL_DOWN, down);
        } else if (sym == SDLK_PAGEUP) {
            nk_input_key(ctx, .NK_KEY_SCROLL_UP, down);
        } else if (sym == SDLK_z) {
            nk_input_key(ctx, .NK_KEY_TEXT_UNDO, down & @intCast(c_int, state[SDL_SCANCODE_LCTRL]));
        } else if (sym == SDLK_r) {
            nk_input_key(ctx, .NK_KEY_TEXT_REDO, down & @intCast(c_int, state[SDL_SCANCODE_LCTRL]));
        } else if (sym == SDLK_c) {
            nk_input_key(ctx, .NK_KEY_COPY, down & @intCast(c_int, state[SDL_SCANCODE_LCTRL]));
        } else if (sym == SDLK_v) {
            nk_input_key(ctx, .NK_KEY_PASTE, down & @intCast(c_int, state[SDL_SCANCODE_LCTRL]));
        } else if (sym == SDLK_x) {
            nk_input_key(ctx, .NK_KEY_CUT, down & @intCast(c_int, state[SDL_SCANCODE_LCTRL]));
        } else if (sym == SDLK_b) {
            nk_input_key(ctx, .NK_KEY_TEXT_LINE_START, down & @intCast(c_int, state[SDL_SCANCODE_LCTRL]));
        } else if (sym == SDLK_e) {
            nk_input_key(ctx, .NK_KEY_TEXT_LINE_END, down & @intCast(c_int, state[SDL_SCANCODE_LCTRL]));
        } else if (sym == SDLK_UP) {
            nk_input_key(ctx, .NK_KEY_UP, down);
        } else if (sym == SDLK_DOWN) {
            nk_input_key(ctx, .NK_KEY_DOWN, down);
        } else if (sym == SDLK_LEFT) {
            if (state[SDL_SCANCODE_LCTRL] != 0) {
                nk_input_key(ctx, .NK_KEY_TEXT_WORD_LEFT, down);
            } else {
                nk_input_key(ctx, .NK_KEY_LEFT, down);
            }
        } else if (sym == SDLK_RIGHT) {
            if (state[SDL_SCANCODE_LCTRL] != 0) {
                nk_input_key(ctx, .NK_KEY_TEXT_WORD_RIGHT, down);
            } else {
                nk_input_key(ctx, .NK_KEY_RIGHT, down);
            }
        } else return 0;
        return 1;
    } else if (evt.@"type" == SDL_MOUSEBUTTONDOWN or evt.@"type" == SDL_MOUSEBUTTONUP) {
        // mouse button
        const down: c_int = if (evt.@"type" == SDL_MOUSEBUTTONDOWN) 1 else 0;
        const x: c_int = @intCast(c_int, evt.button.x);
        const y: c_int = @intCast(c_int, evt.button.y);
        if (evt.button.button == SDL_BUTTON_LEFT) {
            if (evt.button.clicks > 1) {
                nk_input_button(ctx, .NK_BUTTON_DOUBLE, x, y, down);
            }
            nk_input_button(ctx, .NK_BUTTON_LEFT, x, y, down);
        } else if (evt.button.button == SDL_BUTTON_MIDDLE) {
            nk_input_button(ctx, .NK_BUTTON_MIDDLE, x, y, down);
        } else if (evt.button.button == SDL_BUTTON_RIGHT) {
            nk_input_button(ctx, .NK_BUTTON_RIGHT, x, y, down);
        }
        return 1;
    } else if (evt.@"type" == SDL_MOUSEMOTION) {
        // mouse motion
        if (ctx.input.mouse.grabbed != 0) {
            const x: c_int = @floatToInt(c_int, ctx.input.mouse.prev.x);
            const y: c_int = @floatToInt(c_int, ctx.input.mouse.prev.y);
            nk_input_motion(ctx, x + evt.motion.xrel, y + evt.motion.yrel);
        } else nk_input_motion(ctx, evt.motion.x, evt.motion.y);
        return 1;
    } else if (evt.@"type" == SDL_TEXTINPUT) {
        // text input
        var glyph: nk_glyph = undefined;
        _ = memcpy(&glyph, &evt.text.text, NK_UTF_SIZE);
        nk_input_glyph(ctx, &glyph);
        return 1;
    } else if (evt.@"type" == SDL_MOUSEWHEEL) {
        // mouse wheel
        nk_input_scroll(ctx,nk_vec2(@intToFloat(f32, evt.wheel.x), @intToFloat(f32, evt.wheel.y)));
        return 1;
    }
    return 0;
}

fn nk_sdl_shutdown() void
{
    nk_font_atlas_clear(&sdl.atlas);
    nk_free(&sdl.ctx);
    nk_sdl_device_destroy();
    _ = memset(&sdl, 0, @sizeOf(nk_sdl));
}

fn set_style(ctx: *nk_context) void {
    var table: [NK_COLOR_COUNT]nk_color = undefined;
    var i: usize = 0;
    while (i < NK_COLOR_COUNT) : (i += 1) {
        table[i] = nk_rgba(0,0,0,0);
    }
    table[NK_COLOR_TEXT] = nk_rgba(190, 190, 190, 255);
    table[NK_COLOR_WINDOW] = nk_rgba(30, 33, 40, 215);
    table[NK_COLOR_HEADER] = nk_rgba(181, 45, 69, 220);
    table[NK_COLOR_BORDER] = nk_rgba(51, 55, 67, 255);
    table[NK_COLOR_BUTTON] = nk_rgba(181, 45, 69, 255);
    table[NK_COLOR_BUTTON_HOVER] = nk_rgba(190, 50, 70, 255);
    table[NK_COLOR_BUTTON_ACTIVE] = nk_rgba(195, 55, 75, 255);
    table[NK_COLOR_TOGGLE] = nk_rgba(51, 55, 67, 255);
    table[NK_COLOR_TOGGLE_HOVER] = nk_rgba(45, 60, 60, 255);
    table[NK_COLOR_TOGGLE_CURSOR] = nk_rgba(181, 45, 69, 255);
    table[NK_COLOR_SELECT] = nk_rgba(51, 55, 67, 255);
    table[NK_COLOR_SELECT_ACTIVE] = nk_rgba(181, 45, 69, 255);
    table[NK_COLOR_SLIDER] = nk_rgba(51, 55, 67, 255);
    table[NK_COLOR_SLIDER_CURSOR] = nk_rgba(181, 45, 69, 255);
    table[NK_COLOR_SLIDER_CURSOR_HOVER] = nk_rgba(186, 50, 74, 255);
    table[NK_COLOR_SLIDER_CURSOR_ACTIVE] = nk_rgba(191, 55, 79, 255);
    table[NK_COLOR_PROPERTY] = nk_rgba(51, 55, 67, 255);
    table[NK_COLOR_EDIT] = nk_rgba(51, 55, 67, 225);
    table[NK_COLOR_EDIT_CURSOR] = nk_rgba(190, 190, 190, 255);
    table[NK_COLOR_COMBO] = nk_rgba(51, 55, 67, 255);
    table[NK_COLOR_CHART] = nk_rgba(51, 55, 67, 255);
    table[NK_COLOR_CHART_COLOR] = nk_rgba(170, 40, 60, 255);
    table[NK_COLOR_CHART_COLOR_HIGHLIGHT] = nk_rgba( 255, 0, 0, 255);
    table[NK_COLOR_SCROLLBAR] = nk_rgba(30, 33, 40, 255);
    table[NK_COLOR_SCROLLBAR_CURSOR] = nk_rgba(64, 84, 95, 255);
    table[NK_COLOR_SCROLLBAR_CURSOR_HOVER] = nk_rgba(70, 90, 100, 255);
    table[NK_COLOR_SCROLLBAR_CURSOR_ACTIVE] = nk_rgba(75, 95, 105, 255);
    table[NK_COLOR_TAB_HEADER] = nk_rgba(181, 45, 69, 220);
    nk_style_from_table(ctx, &table);
}
