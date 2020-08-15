const focus = @import("../focus.zig");
usingnamespace focus.common;
const meta = focus.meta;
const Atlas = focus.Atlas;
const App = focus.App;
const Id = focus.Id;
const Editor = focus.Editor;
const FileOpener = focus.FileOpener;
const ProjectFileOpener = focus.ProjectFileOpener;
const ProjectSearcher = focus.ProjectSearcher;
const style = focus.style;

pub const Window = struct {
    app: *App,
    // views.len > 0
    views: ArrayList(Id),

    sdl_window: *c.SDL_Window,
    width: Coord,
    height: Coord,

    gl_context: c.SDL_GLContext,
    texture_buffer: ArrayList(Quad(Vec2f)),
    vertex_buffer: ArrayList(Quad(Vec2f)),
    color_buffer: ArrayList(Quad(Color)),
    index_buffer: ArrayList([2]Tri(u32)),

    pub fn init(app: *App, view: Id) Id {
        var views = ArrayList(Id).init(app.allocator);
        views.append(view) catch oom();

        // pretty arbitrary
        const init_width: usize = 1920;
        const init_height: usize = 1080;

        // init window
        const sdl_window = c.SDL_CreateWindow(
            "focus",
            c.SDL_WINDOWPOS_UNDEFINED,
            c.SDL_WINDOWPOS_UNDEFINED,
            @as(c_int, init_width),
            @as(c_int, init_height),
            c.SDL_WINDOW_OPENGL | c.SDL_WINDOW_BORDERLESS | c.SDL_WINDOW_ALLOW_HIGHDPI | c.SDL_WINDOW_RESIZABLE,
        ) orelse panic("SDL window creation failed: {s}", .{c.SDL_GetError()});

        // TODO zig compiler can't handle this macro-fest yet
        //var info: c.SDL_SysWMinfo = undefined;
        //c.SDL_VERSION(&info.version);
        //if (!SDL_GetWindowWMInfo(sdl_window, &info)) {
        //   panic("Could not get window info: {s}", .{c.SDL_GetError()});
        //}
        //if (info.subsystem != c.SDL_SYSWM_WAYLAND) {
        //    panic("Wanted wayland, got subsystem={}", .{info.subsystem});
        //}

        // init gl
        const gl_context = c.SDL_GL_CreateContext(sdl_window);
        if (c.SDL_GL_MakeCurrent(sdl_window, gl_context) != 0)
            panic("Switching to GL context failed: {s}", .{c.SDL_GetError()});
        c.glEnable(c.GL_BLEND);
        c.glBlendFunc(c.GL_SRC_ALPHA, c.GL_ONE_MINUS_SRC_ALPHA);
        c.glDisable(c.GL_CULL_FACE);
        c.glDisable(c.GL_DEPTH_TEST);
        c.glEnable(c.GL_TEXTURE_2D);
        c.glEnableClientState(c.GL_VERTEX_ARRAY);
        c.glEnableClientState(c.GL_TEXTURE_COORD_ARRAY);
        c.glEnableClientState(c.GL_COLOR_ARRAY);

        // init texture
        // TODO should this be per-window or per-app?
        var id: u32 = undefined;
        c.glGenTextures(1, &id);
        c.glBindTexture(c.GL_TEXTURE_2D, id);
        c.glTexImage2D(c.GL_TEXTURE_2D, 0, c.GL_ALPHA, app.atlas.texture_dims.x, app.atlas.texture_dims.y, 0, c.GL_RGBA, c.GL_UNSIGNED_BYTE, app.atlas.texture.ptr);
        c.glTexParameteri(c.GL_TEXTURE_2D, c.GL_TEXTURE_MIN_FILTER, c.GL_NEAREST);
        c.glTexParameteri(c.GL_TEXTURE_2D, c.GL_TEXTURE_MAG_FILTER, c.GL_NEAREST);
        assert(c.glGetError() == 0);

        // no vsync - causes problems with multiple windows
        // see https://stackoverflow.com/questions/29617370/multiple-opengl-contexts-multiple-windows-multithreading-and-vsync
        if (c.SDL_GL_SetSwapInterval(0) != 0)
            panic("Setting swap interval failed: {}", .{c.SDL_GetError()});

        // accept unicode input
        // TODO does this need to be per window?
        c.SDL_StartTextInput();

        // TODO ignore MOUSEMOTION since we just look at current state
        // c.SDL_EventState( c.SDL_MOUSEMOTION, c.SDL_IGNORE );

        return app.putThing(Window{
            .app = app,
            .views = views,

            .sdl_window = sdl_window,
            .width = init_width,
            .height = init_height,

            .gl_context = gl_context,
            .texture_buffer = ArrayList(Quad(Vec2f)).init(app.allocator),
            .vertex_buffer = ArrayList(Quad(Vec2f)).init(app.allocator),
            .color_buffer = ArrayList(Quad(Color)).init(app.allocator),
            .index_buffer = ArrayList([2]Tri(u32)).init(app.allocator),
        });
    }

    pub fn deinit(self: *Window) void {
        self.index_buffer.deinit();
        self.color_buffer.deinit();
        self.vertex_buffer.deinit();
        self.texture_buffer.deinit();
        c.SDL_GL_DeleteContext(self.gl_context);
        c.SDL_DestroyWindow(self.sdl_window);
    }

    pub fn frame(self: *Window, events: []const c.SDL_Event) void {
        // figure out window size
        var w: c_int = undefined;
        var h: c_int = undefined;
        c.SDL_GL_GetDrawableSize(self.sdl_window, &w, &h);
        self.width = @intCast(Coord, w);
        self.height = @intCast(Coord, h);
        const window_rect = Rect{ .x = 0, .y = 0, .w = self.width, .h = self.height };

        var view_events = ArrayList(c.SDL_Event).init(self.app.frame_allocator);

        // handle events
        for (events) |event| {
            var handled = false;
            switch (event.type) {
                c.SDL_KEYDOWN => {
                    const sym = event.key.keysym;
                    if (sym.mod == c.KMOD_LCTRL or sym.mod == c.KMOD_RCTRL) {
                        switch (sym.sym) {
                            'q' => {
                                switch (self.app.getThing(self.views.items[self.views.items.len - 1])) {
                                    .Editor => {},
                                    else => self.popView(),
                                }
                            },
                            'o' => {
                                var init_path: []const u8 = "/home/jamie/";
                                switch (self.app.getThing(self.views.items[self.views.items.len - 1])) {
                                    .Editor => |editor| {
                                        const buffer = self.app.getThing(editor.buffer_id).Buffer;
                                        switch (buffer.source) {
                                            .None => {},
                                            .AbsoluteFilename => |filename| {
                                                init_path = std.mem.concat(self.app.frame_allocator, u8, &[_][]const u8{ std.fs.path.dirname(filename).?, "/" }) catch oom();
                                            },
                                        }
                                    },
                                    else => {},
                                }
                                const file_opener_id = FileOpener.init(self.app, init_path);
                                self.pushView(file_opener_id);
                                handled = true;
                            },
                            'p' => {
                                const project_file_opener_id = ProjectFileOpener.init(self.app);
                                self.pushView(project_file_opener_id);
                                handled = true;
                            },
                            'n' => {
                                switch (self.app.getThing(self.views.items[self.views.items.len - 1])) {
                                    .Editor => |editor| {
                                        const new_editor_id = Editor.init(self.app, editor.buffer_id);
                                        const new_window_id = Window.init(self.app, new_editor_id);
                                    },
                                    else => {},
                                }
                                handled = true;
                            },
                            else => {},
                        }
                    }
                    if (sym.mod == c.KMOD_LALT or sym.mod == c.KMOD_RALT) {
                        switch (sym.sym) {
                            'f' => {
                                var project_dir: []const u8 = "/home/jamie";
                                var filter: []const u8 = "";
                                switch (self.app.getThing(self.views.items[self.views.items.len - 1])) {
                                    .Editor => |editor| {
                                        const buffer = self.app.getThing(editor.buffer_id).Buffer;
                                        switch (buffer.source) {
                                            .None => {},
                                            .AbsoluteFilename => |filename| {
                                                const dirname = std.fs.path.dirname(filename).?;
                                                var root = dirname;
                                                while (!meta.deepEqual(root, "/")) {
                                                    const git_path = std.fs.path.join(self.app.frame_allocator, &[2][]const u8{ root, ".git" }) catch oom();
                                                    if (std.fs.openFileAbsolute(git_path, .{})) |file| {
                                                        file.close();
                                                        break;
                                                    } else |_| {}
                                                    root = std.fs.path.dirname(root).?;
                                                }
                                                project_dir = if (meta.deepEqual(root, "/")) dirname else root;
                                                filter = editor.dupeSelection(self.app.frame_allocator, editor.getMainCursor());
                                            },
                                        }
                                    },
                                    else => {},
                                }
                                const project_searcher_id = ProjectSearcher.init(self.app, project_dir, filter);
                                self.pushView(project_searcher_id);
                                handled = true;
                            },
                            else => {},
                        }
                    }
                },
                c.SDL_WINDOWEVENT => {
                    switch (event.window.event) {
                        c.SDL_WINDOWEVENT_FOCUS_LOST => {
                            switch (self.app.getThing(self.views.items[self.views.items.len - 1])) {
                                .Editor => |editor| editor.save(),
                                else => {},
                            }
                            handled = true;
                        },
                        c.SDL_WINDOWEVENT_CLOSE => {
                            self.app.removeThing(self);
                            self.deinit();
                            return;
                        },
                        else => {},
                    }
                },
                else => {},
            }
            // delegate other events to editor
            if (!handled) view_events.append(event) catch oom();
        }

        // run view frame
        var view = self.app.getThing(self.views.items[self.views.items.len - 1]);
        switch (view) {
            .Editor => |editor| editor.frame(self, window_rect, view_events.items),
            .FileOpener => |file_opener| file_opener.frame(self, window_rect, view_events.items),
            .ProjectFileOpener => |project_file_opener| project_file_opener.frame(self, window_rect, view_events.items),
            .BufferSearcher => |buffer_searcher| buffer_searcher.frame(self, window_rect, view_events.items),
            .ProjectSearcher => |project_searcher| project_searcher.frame(self, window_rect, view_events.items),
            else => panic("Not a view: {}", .{view}),
        }

        // render
        if (c.SDL_GL_MakeCurrent(self.sdl_window, self.gl_context) != 0)
            panic("Switching to GL context failed: {s}", .{c.SDL_GetError()});

        c.glClearColor(0, 0, 0, 1);
        c.glClear(c.GL_COLOR_BUFFER_BIT);

        c.glViewport(0, 0, self.width, self.height);
        c.glMatrixMode(c.GL_PROJECTION);
        c.glPushMatrix();
        c.glLoadIdentity();
        c.glOrtho(0.0, @intToFloat(f32, self.width), @intToFloat(f32, self.height), 0.0, -1.0, 1.0);
        c.glMatrixMode(c.GL_MODELVIEW);
        c.glPushMatrix();
        c.glLoadIdentity();

        c.glTexCoordPointer(2, c.GL_FLOAT, 0, self.texture_buffer.items.ptr);
        c.glVertexPointer(2, c.GL_FLOAT, 0, self.vertex_buffer.items.ptr);
        c.glColorPointer(4, c.GL_UNSIGNED_BYTE, 0, self.color_buffer.items.ptr);
        c.glDrawElements(c.GL_TRIANGLES, @intCast(c_int, self.index_buffer.items.len) * 6, c.GL_UNSIGNED_INT, self.index_buffer.items.ptr);

        c.glMatrixMode(c.GL_MODELVIEW);
        c.glPopMatrix();
        c.glMatrixMode(c.GL_PROJECTION);
        c.glPopMatrix();

        c.SDL_GL_SwapWindow(self.sdl_window);

        // reset
        self.texture_buffer.resize(0) catch oom();
        self.vertex_buffer.resize(0) catch oom();
        self.color_buffer.resize(0) catch oom();
        self.index_buffer.resize(0) catch oom();
    }

    fn queueQuad(self: *Window, dst: Rect, src: Rect, color: Color) void {
        const tx = @intToFloat(f32, src.x) / @intToFloat(f32, self.app.atlas.texture_dims.x);
        const ty = @intToFloat(f32, src.y) / @intToFloat(f32, self.app.atlas.texture_dims.y);
        const tw = @intToFloat(f32, src.w) / @intToFloat(f32, self.app.atlas.texture_dims.x);
        const th = @intToFloat(f32, src.h) / @intToFloat(f32, self.app.atlas.texture_dims.y);
        self.texture_buffer.append(.{
            .tl = .{ .x = tx, .y = ty },
            .tr = .{ .x = tx + tw, .y = ty },
            .bl = .{ .x = tx, .y = ty + th },
            .br = .{ .x = tx + tw, .y = ty + th },
        }) catch oom();

        const vx = @intToFloat(f32, dst.x);
        const vy = @intToFloat(f32, dst.y);
        const vw = @intToFloat(f32, dst.w);
        const vh = @intToFloat(f32, dst.h);
        self.vertex_buffer.append(.{
            .tl = .{ .x = vx, .y = vy },
            .tr = .{ .x = vx + vw, .y = vy },
            .bl = .{ .x = vx, .y = vy + vh },
            .br = .{ .x = vx + vw, .y = vy + vh },
        }) catch oom();

        self.color_buffer.append(.{
            .tl = color,
            .tr = color,
            .bl = color,
            .br = color,
        }) catch oom();

        const vertex_ix = @intCast(u32, self.index_buffer.items.len * 4);
        self.index_buffer.append(.{
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
        }) catch oom();
    }

    // view api

    // TODO instead of ids, put popped views onto a todo list and free at the end of the frame

    pub fn pushView(self: *Window, view: Id) void {
        switch (self.app.getThing(self.views.items[self.views.items.len - 1])) {
            .Editor => |editor| editor.save(),
            else => {},
        }
        self.views.append(view) catch oom();
    }

    pub fn popView(self: *Window) void {
        const view_id = self.views.pop();
        switch (self.app.getThing(view_id)) {
            .Editor => |editor| editor.save(),
            else => {},
        }
    }

    // drawing api

    pub fn queueRect(self: *Window, rect: Rect, color: Color) void {
        self.queueQuad(rect, self.app.atlas.white_rect, color);
    }

    pub fn queueText(self: *Window, pos: Vec2, color: Color, chars: []const u8) void {
        // TODO going to need to be able to clip text
        var dst: Rect = .{ .x = pos.x, .y = pos.y, .w = 0, .h = 0 };
        for (chars) |char| {
            const src = if (char < self.app.atlas.char_to_rect.len)
                self.app.atlas.char_to_rect[char]
            else
            // TODO tofu
                self.app.atlas.white_rect;
            dst.w = src.w;
            dst.h = src.h;
            self.queueQuad(dst, src, color);
            dst.x += src.w;
        }
    }

    // pub fn text(self: *Window, rect: Rect, color: Color, chars: []const u8) void {
    //     var h: Coord = 0;
    //     var line_begin: usize = 0;
    //     while (true) {
    //         var line_end = line_begin;
    //         {
    //             var w: Coord = 0;
    //             var i: usize = line_end;
    //             while (true) {
    //                 if (i >= chars.len) {
    //                     line_end = i;
    //                     break;
    //                 }
    //                 const char = chars[i];
    //                 w += @intCast(Coord, app.atlas.max_char_width);
    //                 if (w > rect.w) {
    //                     // if haven't soft wrapped yet, hard wrap before this char
    //                     if (line_end == line_begin) {
    //                         line_end = i;
    //                     }
    //                     break;
    //                 }
    //                 if (char == '\n') {
    //                     // commit to drawing this char and wrap here
    //                     line_end = i + 1;
    //                     break;
    //                 }
    //                 if (char == ' ') {
    //                     // commit to drawing this char
    //                     line_end = i + 1;
    //                 }
    //                 // otherwise keep looking ahead
    //                 i += 1;
    //             }
    //         }
    //         self.queueText(.{ .x = rect.x, .y = rect.y + h }, color, chars[line_begin..line_end]);
    //         line_begin = line_end;
    //         h += atlas.text_height;
    //         if (line_begin >= chars.len or h > rect.h) {
    //             break;
    //         }
    //     }
    // }

    // util

    pub const SearcherLayout = struct {
        preview: Rect,
        selector: Rect,
        input: Rect,
    };

    pub fn layoutSearcher(self: *Window, rect: Rect) SearcherLayout {
        var all_rect = rect;
        const preview_rect = all_rect.splitTop(@divTrunc(rect.h, 2), 0);
        const border1_rect = all_rect.splitTop(@divTrunc(self.app.atlas.char_height, 8), 0);
        const input_rect = all_rect.splitBottom(self.app.atlas.char_height, 0);
        const border2_rect = all_rect.splitBottom(@divTrunc(self.app.atlas.char_height, 8), 0);
        const selector_rect = all_rect;
        self.queueRect(border1_rect, style.text_color);
        self.queueRect(border2_rect, style.text_color);
        return .{ .preview = preview_rect, .selector = selector_rect, .input = input_rect };
    }
};
