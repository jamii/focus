usingnamespace @import("common.zig");

const max_vertex_memory = 512 * 1024;
const max_element_memory = 128 * 1024;

const SdlPlumbing = struct {
    win: *SDL_Window,
    ctx: SDL_GLContext,

    fn init() SdlPlumbing {
        _ = SDL_SetHint(SDL_HINT_VIDEO_HIGHDPI_DISABLED, "0");
        _ = SDL_GL_SetAttribute (.SDL_GL_CONTEXT_FLAGS, SDL_GL_CONTEXT_FORWARD_COMPATIBLE_FLAG);
        _ = SDL_GL_SetAttribute (.SDL_GL_CONTEXT_PROFILE_MASK, SDL_GL_CONTEXT_PROFILE_CORE);
        _ = SDL_GL_SetAttribute(.SDL_GL_CONTEXT_MAJOR_VERSION, 3);
        _ = SDL_GL_SetAttribute(.SDL_GL_CONTEXT_MINOR_VERSION, 3);
        _ = SDL_GL_SetAttribute(.SDL_GL_DOUBLEBUFFER, 1);
        const win = SDL_CreateWindow(
            "Demo",
            SDL_WINDOWPOS_CENTERED, SDL_WINDOWPOS_CENTERED,
            window_width, window_height, SDL_WINDOW_OPENGL|SDL_WINDOW_SHOWN|SDL_WINDOW_ALLOW_HIGHDPI
        ).?;
        const ctx = SDL_GL_CreateContext(win);
        glViewport(0, 0, window_width, window_height);
        return SdlPlumbing{.win = win, .ctx = ctx};
    }

    fn deinit(self: *SdlPlumbing) void {
        SDL_GL_DeleteContext(self.ctx);
        SDL_DestroyWindow(self.win);
        SDL_Quit();
    }
};

const GlPlumbing = struct {
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

    fn init() GlPlumbing {
        var prog = glCreateProgram();

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
        const vert_shdr = glCreateShader(GL_VERTEX_SHADER);
        const frag_shdr = glCreateShader(GL_FRAGMENT_SHADER);
        glShaderSource(vert_shdr, 1, &vertex_shader, 0);
        glShaderSource(frag_shdr, 1, &fragment_shader, 0);
        glCompileShader(vert_shdr);
        glCompileShader(frag_shdr);
        glGetShaderiv(vert_shdr, GL_COMPILE_STATUS, &status);
        assert(status == GL_TRUE);
        glGetShaderiv(frag_shdr, GL_COMPILE_STATUS, &status);
        assert(status == GL_TRUE);
        glAttachShader(prog, vert_shdr);
        glAttachShader(prog, frag_shdr);
        glLinkProgram(prog);
        glGetProgramiv(prog, GL_LINK_STATUS, &status);
        assert(status == GL_TRUE);

        const uniform_tex = glGetUniformLocation(prog, "Texture");
        const uniform_proj = glGetUniformLocation(prog, "ProjMtx");
        const attrib_pos = glGetAttribLocation(prog, "Position");
        const attrib_uv = glGetAttribLocation(prog, "TexCoord");
        const attrib_col = glGetAttribLocation(prog, "Color");
        var font_tex: GLuint = undefined;
        glGenTextures(1, &font_tex);
        const vs = @sizeOf(SdlVertex);
        const vp = @byteOffsetOf(SdlVertex, "position");
        const vt = @byteOffsetOf(SdlVertex, "uv");
        const vc = @byteOffsetOf(SdlVertex, "col");

        var vbo: GLuint = undefined;
        var ebo: GLuint = undefined;
        glGenBuffers(1, &vbo);
        glGenBuffers(1, &ebo);

        glBindTexture(GL_TEXTURE_2D, 0);
        glBindBuffer(GL_ARRAY_BUFFER, 0);
        glBindBuffer(GL_ELEMENT_ARRAY_BUFFER, 0);

        return GlPlumbing {
            .vbo = vbo,
            .ebo = ebo,
            .prog = prog,
            .vert_shdr = vert_shdr,
            .frag_shdr = frag_shdr,
            .attrib_pos = attrib_pos,
            .attrib_uv = attrib_uv,
            .attrib_col = attrib_col,
            .uniform_tex = uniform_tex,
            .uniform_proj = uniform_proj,
            .font_tex = font_tex,
            .vs = vs,
            .vp = vp,
            .vt = vt,
            .vc = vc,
        };
    }

    fn deinit(self: *GlPlumbing) void {
        glDetachShader(self.prog, self.vert_shdr);
        glDetachShader(self.prog, self.frag_shdr);
        glDeleteShader(self.vert_shdr);
        glDeleteShader(self.frag_shdr);
        glDeleteProgram(self.prog);
        glDeleteTextures(1, &self.font_tex);
        glDeleteBuffers(1, &self.vbo);
        glDeleteBuffers(1, &self.ebo);
    }
};

const NkPlumbing = struct {
    cmds: nk_buffer,
    ctx: nk_context,
    null_texture: nk_draw_null_texture,
    atlas: nk_font_atlas,

    fn init(gl: *const GlPlumbing, sdl: *const SdlPlumbing) NkPlumbing {
        var cmds: nk_buffer = undefined;
        {
            nk_buffer_init_default(&cmds);
        }

        var ctx: nk_context = undefined;
        {
            _ = nk_init_default(&ctx, 0);
            ctx.clip.copy = clipboard_copy;
            ctx.clip.paste = clipboard_paste;
            ctx.clip.userdata = nk_handle_ptr(null);
        }

        var atlas: nk_font_atlas = undefined;
        var null_texture: nk_draw_null_texture = undefined;
        {
            nk_font_atlas_init_default(&atlas);
            nk_font_atlas_begin(&atlas);
            // add fonts here
            var width: c_int = undefined;
            var height: c_int = undefined;
            const image: *const c_void = nk_font_atlas_bake(&atlas, &width, &height, .NK_FONT_ATLAS_RGBA32).?;
            glBindTexture(GL_TEXTURE_2D, gl.font_tex);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR);
            glTexImage2D(GL_TEXTURE_2D, 0, GL_RGBA, @intCast(GLsizei, width), @intCast(GLsizei, height), 0,
                         GL_RGBA, GL_UNSIGNED_BYTE, image);
            nk_font_atlas_end(&atlas, nk_handle_id(@intCast(c_int, gl.font_tex)), &null_texture);
            if (atlas.default_font != null) {
                nk_style_set_font(&ctx, &atlas.default_font.*.handle);
            }
        }

        // style
        {
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
            nk_style_from_table(&ctx, &table);
        }

        return NkPlumbing{
            .cmds = cmds,
            .ctx = ctx,
            .null_texture = null_texture,
            .atlas = atlas,
        };
    }

    fn deinit(self: *NkPlumbing) void {
        nk_buffer_free(&self.cmds);
        nk_font_atlas_clear(&self.atlas);
        nk_free(&self.ctx);
    }
};

pub const Plumbing = struct {
    sdl: SdlPlumbing,
    gl: GlPlumbing,
    nk: NkPlumbing,

    pub fn init() Plumbing {
        const sdl = SdlPlumbing.init();
        const gl = GlPlumbing.init();
        const nk = NkPlumbing.init(&gl, &sdl);
        return Plumbing {
            .sdl = sdl,
            .gl = gl,
            .nk = nk,
        };
    }

    pub fn deinit(self: *Plumbing) void {
        self.nk.deinit();
        self.gl.deinit();
        self.sdl.deinit();
    }

    pub fn handle_input(self: *Plumbing, is_running: *bool) void {
        var ctx = &self.nk.ctx;
        nk_input_begin(ctx);
        var evt: SDL_Event = undefined;
        while (nk_true == SDL_PollEvent(&evt)) {
            if (evt.type == SDL_QUIT) {
                is_running.* = false;
            } else if (evt.@"type" == SDL_KEYUP or evt.@"type" == SDL_KEYDOWN) {
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
                }
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
            } else if (evt.@"type" == SDL_MOUSEMOTION) {
                // mouse motion
                if (ctx.input.mouse.grabbed != 0) {
                    const x: c_int = @floatToInt(c_int, ctx.input.mouse.prev.x);
                    const y: c_int = @floatToInt(c_int, ctx.input.mouse.prev.y);
                    nk_input_motion(ctx, x + evt.motion.xrel, y + evt.motion.yrel);
                } else {
                    nk_input_motion(ctx, evt.motion.x, evt.motion.y);
                }
            } else if (evt.@"type" == SDL_TEXTINPUT) {
                // text input
                var glyph: nk_glyph = undefined;
                _ = memcpy(&glyph, &evt.text.text, NK_UTF_SIZE);
                nk_input_glyph(ctx, &glyph);
            } else if (evt.@"type" == SDL_MOUSEWHEEL) {
                // mouse wheel
                nk_input_scroll(ctx, nk_vec2(@intToFloat(f32, evt.wheel.x), @intToFloat(f32, evt.wheel.y)));
            }
        }
        nk_input_end(ctx);
    }

    pub fn draw(self: *Plumbing) void {

        // background
        var bg: [4]f32 = .{0, 0, 0, 0};
        nk_color_fv(&bg, nk_rgb(28,48,62));
        var win_width: c_int = undefined;
        var win_height: c_int = undefined;
        SDL_GetWindowSize(self.sdl.win, &win_width, &win_height);
        glViewport(0, 0, win_width, win_height);
        glClear(GL_COLOR_BUFFER_BIT);
        glClearColor(bg[0], bg[1], bg[2], bg[3]);

        // projection
        var width: c_int = undefined;
        var height: c_int = undefined;
        SDL_GetWindowSize(self.sdl.win, &width, &height);
        var display_width: c_int = undefined;
        var display_height: c_int = undefined;
        SDL_GL_GetDrawableSize(self.sdl.win, &display_width, &display_height);
        var ortho: [4][4]GLfloat = .{
            .{2.0, 0.0, 0.0, 0.0},
            .{0.0,-2.0, 0.0, 0.0},
            .{0.0, 0.0,-1.0, 0.0},
            .{-1.0,1.0, 0.0, 1.0},
        };
        ortho[0][0] /= @intToFloat(GLfloat, width);
        ortho[1][1] /= @intToFloat(GLfloat, height);
        var scale: struct_nk_vec2 = .{
            .x = @intToFloat(f32, display_width) / @intToFloat(f32, width),
            .y = @intToFloat(f32, display_height) / @intToFloat(f32, height),
            .dummy = @splat(16, @intCast(u8, 0)),
        };

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
        glUseProgram(self.gl.prog);
        glUniform1i(self.gl.uniform_tex, 0);
        glUniformMatrix4fv(self.gl.uniform_proj, 1, GL_FALSE, &ortho[0][0]);

        // bind buffers
        glBindBuffer(GL_ARRAY_BUFFER, self.gl.vbo);
        glBindBuffer(GL_ELEMENT_ARRAY_BUFFER, self.gl.ebo);

        // buffer setup
        glEnableVertexAttribArray(@intCast(GLuint, self.gl.attrib_pos));
        glEnableVertexAttribArray(@intCast(GLuint, self.gl.attrib_uv));
        glEnableVertexAttribArray(@intCast(GLuint, self.gl.attrib_col));
        glVertexAttribPointer(@intCast(GLuint, self.gl.attrib_pos), 2, GL_FLOAT, GL_FALSE, self.gl.vs, @intToPtr(?*c_void, self.gl.vp));
        glVertexAttribPointer(@intCast(GLuint, self.gl.attrib_uv), 2, GL_FLOAT, GL_FALSE, self.gl.vs, @intToPtr(?*c_void, self.gl.vt));
        glVertexAttribPointer(@intCast(GLuint, self.gl.attrib_col), 4, GL_UNSIGNED_BYTE, GL_TRUE, self.gl.vs, @intToPtr(?*c_void, self.gl.vc));
        glBufferData(GL_ARRAY_BUFFER, max_vertex_memory, NULL, GL_STREAM_DRAW);
        glBufferData(GL_ELEMENT_ARRAY_BUFFER, max_element_memory, NULL, GL_STREAM_DRAW);

        // load vertices/elements directly into vertex/element buffer
        var vertices: *c_void = undefined;
        var elements: *c_void = undefined;
        vertices = malloc(@intCast(usize, max_vertex_memory));
        elements = malloc(@intCast(usize, max_element_memory));

        // fill convert configuration
        var config: nk_convert_config = undefined;
        const vertex_layout: [4]nk_draw_vertex_layout_element = .{
            .{
                .attribute = .NK_VERTEX_POSITION,
                .format = .NK_FORMAT_FLOAT,
                .offset = @byteOffsetOf(SdlVertex, "position"),
            },
            .{
                .attribute = .NK_VERTEX_TEXCOORD,
                .format = .NK_FORMAT_FLOAT,
                .offset = @byteOffsetOf(SdlVertex, "uv"),
            },
            .{
                .attribute = .NK_VERTEX_COLOR,
                .format = .NK_FORMAT_R8G8B8A8,
                .offset = @byteOffsetOf(SdlVertex, "col"),
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
        config.vertex_size = @sizeOf(SdlVertex);
        config.vertex_alignment = @alignOf(SdlVertex);
        config.@"null" = self.nk.null_texture;
        config.circle_segment_count = 22;
        config.curve_segment_count = 22;
        config.arc_segment_count = 22;
        config.global_alpha = 1.0;
        config.shape_AA = .NK_ANTI_ALIASING_ON;
        config.line_AA = .NK_ANTI_ALIASING_ON;

        // setup buffers to load vertices and elements
        var vbuf: nk_buffer = undefined;
        var ebuf: nk_buffer = undefined;
        nk_buffer_init_fixed(&vbuf, vertices, @intCast(nk_size, max_vertex_memory));
        nk_buffer_init_fixed(&ebuf, elements, @intCast(nk_size, max_element_memory));
        _ = nk_convert(&self.nk.ctx, &self.nk.cmds, &vbuf, &ebuf, &config);

        glBufferSubData(GL_ARRAY_BUFFER, 0, @intCast(c_long, max_vertex_memory), vertices);
        glBufferSubData(GL_ELEMENT_ARRAY_BUFFER, 0, @intCast(c_long, max_element_memory), elements);
        free(vertices);
        free(elements);

        // iterate over and execute each draw command
        var cmd = nk__draw_begin(&self.nk.ctx, &self.nk.cmds);
        var offset: [*c]nk_draw_index = null;
        while (cmd != 0) : (cmd = nk__draw_next(cmd, &self.nk.cmds, &self.nk.ctx)) {
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
        nk_clear(&self.nk.ctx);

        glUseProgram(0);
        glBindBuffer(GL_ARRAY_BUFFER, 0);
        glBindBuffer(GL_ELEMENT_ARRAY_BUFFER, 0);

        glDisable(GL_BLEND);
        glDisable(GL_SCISSOR_TEST);

        SDL_GL_SwapWindow(self.sdl.win);
    }
};

export fn clipboard_paste(usr: nk_handle, edit: [*c]nk_text_edit) void {
    const text: [*c]const u8= SDL_GetClipboardText();
    if (text != null) {
        _ = nk_textedit_paste(edit, text, nk_strlen(text));
    }
}

export fn clipboard_copy(usr: nk_handle, text: [*c]const u8, len: c_int) void {
    var str: [*c]u8 = undefined;
    if (len == 0) return;
    str = @ptrCast([*c]u8, malloc(@intCast(usize, len+1)));
    if (str == null) return;
    _ = memcpy(str, text, @intCast(usize, len));
    str[@intCast(usize, len)] = 0;
    _ = SDL_SetClipboardText(str);
    free(str);
}

const SdlVertex = extern struct {
    position: [2]GLfloat,
    uv: [2]GLfloat,
    col: [4]nk_byte,
};
