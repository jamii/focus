const focus = @import("../focus.zig");
usingnamespace focus.common;
const meta = focus.meta;
const Atlas = focus.Atlas;
const App = focus.App;
const Editor = focus.Editor;
const FileOpener = focus.FileOpener;
const ProjectFileOpener = focus.ProjectFileOpener;
const BufferOpener = focus.BufferOpener;
const BufferSearcher = focus.BufferSearcher;
const ProjectSearcher = focus.ProjectSearcher;
const Launcher = focus.Launcher;
const style = focus.style;

pub const View = union(enum) {
    Editor: *Editor,
    FileOpener: *FileOpener,
    ProjectFileOpener: *ProjectFileOpener,
    BufferOpener: *BufferOpener,
    BufferSearcher: *BufferSearcher,
    ProjectSearcher: *ProjectSearcher,
    Launcher: *Launcher,
};
pub const ViewTag = @TagType(View);

pub const Window = struct {
    app: *App,
    // views are allowed to have pointers to previous views on the stack
    views: ArrayList(View),
    popped_views: ArrayList(View),
    close_after_frame: bool,

    // client socket who opened this window, need to tell them when we close
    client_address_o: ?focus.Address,

    sdl_window: *c.SDL_Window,
    width: Coord,
    height: Coord,

    gl_context: c.SDL_GLContext,
    texture_buffer: ArrayList(Quad(Vec2f)),
    vertex_buffer: ArrayList(Quad(Vec2f)),
    color_buffer: ArrayList(Quad(Color)),
    index_buffer: ArrayList([2]Tri(u32)),

    pub fn init(
        app: *App,
        floating: enum { Floating, NotFloating },
    ) Window {
        // pretty arbitrary
        const init_width: usize = 1920;
        const init_height: usize = 1080;

        // init window
        const floating_flag = if (floating == .NotFloating) c.SDL_WINDOW_RESIZABLE else 0;
        const flags = c.SDL_WINDOW_OPENGL | c.SDL_WINDOW_BORDERLESS | c.SDL_WINDOW_ALLOW_HIGHDPI | floating_flag;
        //@compileLog(@TypeOf(c.SDL_WINDOW_OPENGL), @TypeOf(c.SDL_WINDOW_BORDERLESS), @TypeOf(c.SDL_WINDOW_ALLOW_HIGHDPI));
        const sdl_window = c.SDL_CreateWindow(
            "focus",
            c.SDL_WINDOWPOS_UNDEFINED,
            c.SDL_WINDOWPOS_UNDEFINED,
            @as(c_int, init_width),
            @as(c_int, init_height),
            @intCast(u32, flags),
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
        Window.loadAtlasTexture(app.atlas);

        // no vsync - causes problems with multiple windows
        // see https://stackoverflow.com/questions/29617370/multiple-opengl-contexts-multiple-windows-multithreading-and-vsync
        if (c.SDL_GL_SetSwapInterval(0) != 0)
            panic("Setting swap interval failed: {}", .{c.SDL_GetError()});

        // accept unicode input
        // TODO does this need to be per window?
        c.SDL_StartTextInput();

        // TODO ignore MOUSEMOTION since we just look at current state
        // c.SDL_EventState( c.SDL_MOUSEMOTION, c.SDL_IGNORE );

        return Window{
            .app = app,
            .views = ArrayList(View).init(app.allocator),
            .popped_views = ArrayList(View).init(app.allocator),
            .close_after_frame = false,

            .client_address_o = null,

            .sdl_window = sdl_window,
            .width = init_width,
            .height = init_height,

            .gl_context = gl_context,
            .texture_buffer = ArrayList(Quad(Vec2f)).init(app.allocator),
            .vertex_buffer = ArrayList(Quad(Vec2f)).init(app.allocator),
            .color_buffer = ArrayList(Quad(Color)).init(app.allocator),
            .index_buffer = ArrayList([2]Tri(u32)).init(app.allocator),
        };
    }

    pub fn loadAtlasTexture(atlas: *Atlas) void {
        var id: u32 = undefined;
        c.glGenTextures(1, &id);
        c.glBindTexture(c.GL_TEXTURE_2D, id);
        c.glTexImage2D(c.GL_TEXTURE_2D, 0, c.GL_ALPHA, atlas.texture_dims.x, atlas.texture_dims.y, 0, c.GL_RGBA, c.GL_UNSIGNED_BYTE, atlas.texture.ptr);
        c.glTexParameteri(c.GL_TEXTURE_2D, c.GL_TEXTURE_MIN_FILTER, c.GL_NEAREST);
        c.glTexParameteri(c.GL_TEXTURE_2D, c.GL_TEXTURE_MAG_FILTER, c.GL_NEAREST);
        assert(c.glGetError() == 0);
    }

    pub fn deinit(self: *Window) void {
        self.index_buffer.deinit();
        self.color_buffer.deinit();
        self.vertex_buffer.deinit();
        self.texture_buffer.deinit();
        c.SDL_GL_DeleteContext(self.gl_context);
        c.SDL_DestroyWindow(self.sdl_window);

        while (self.views.items.len > 0) {
            const view = self.views.pop();
            self.popped_views.append(view) catch oom();
        }
        self.deinitPoppedViews();
        self.popped_views.deinit();
        self.views.deinit();
    }

    fn getTopView(self: *Window) ?View {
        if (self.views.items.len > 0)
            return self.views.items[self.views.items.len - 1]
        else
            return null;
    }

    fn getTopViewIfEditor(self: *Window) ?*Editor {
        if (self.getTopView()) |view| {
            switch (view) {
                .Editor => |editor| return editor,
                else => return null,
            }
        } else return null;
    }

    fn getTopViewFilename(self: *Window) ?[]const u8 {
        if (self.getTopViewIfEditor()) |editor|
            return editor.buffer.getFilename()
        else
            return null;
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
                            'q' => self.popView(),
                            'o' => {
                                const init_path = if (self.getTopViewFilename()) |filename|
                                    std.mem.concat(self.app.frame_allocator, u8, &[_][]const u8{ std.fs.path.dirname(filename).?, "/" }) catch oom()
                                else
                                    "/home/jamie/";
                                const file_opener = FileOpener.init(self.app, init_path);
                                self.pushView(file_opener);
                                handled = true;
                            },
                            'p' => {
                                const project_file_opener = ProjectFileOpener.init(self.app);
                                self.pushView(project_file_opener);
                                handled = true;
                            },
                            'n' => {
                                if (self.getTopViewIfEditor()) |editor| {
                                    const new_window = self.app.registerWindow(Window.init(self.app, .NotFloating));
                                    const new_editor = Editor.init(self.app, editor.buffer, true, true);
                                    new_editor.top_pixel = editor.top_pixel;
                                    new_window.pushView(new_editor);
                                }
                                handled = true;
                            },
                            '-' => {
                                self.app.changeFontSize(-1);
                                handled = true;
                            },
                            '=' => {
                                self.app.changeFontSize(1);
                                handled = true;
                            },
                            else => {},
                        }
                    }
                    if (sym.mod == c.KMOD_LALT or sym.mod == c.KMOD_RALT) {
                        switch (sym.sym) {
                            'f' => {
                                var project_dir: []const u8 = "/home/jamie";
                                if (self.getTopViewIfEditor()) |editor| {
                                    if (editor.buffer.getFilename()) |filename| {
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
                                    }
                                }
                                const project_searcher = ProjectSearcher.init(self.app, project_dir);
                                self.pushView(project_searcher);
                                handled = true;
                            },
                            'p' => {
                                const buffer_opener = BufferOpener.init(self.app);
                                self.pushView(buffer_opener);
                                handled = true;
                            },
                            else => {},
                        }
                    }
                },
                c.SDL_WINDOWEVENT => {
                    switch (event.window.event) {
                        c.SDL_WINDOWEVENT_FOCUS_LOST => {
                            if (self.getTopViewIfEditor()) |editor| editor.save(.Auto);
                            handled = true;
                        },
                        c.SDL_WINDOWEVENT_CLOSE => {
                            self.close_after_frame = true;
                            handled = true;
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
        if (self.getTopView()) |view| {
            switch (view) {
                .Editor => |editor| editor.frame(self, window_rect, view_events.items),
                .FileOpener => |file_opener| file_opener.frame(self, window_rect, view_events.items),
                .ProjectFileOpener => |project_file_opener| project_file_opener.frame(self, window_rect, view_events.items),
                .BufferOpener => |buffer_opener| buffer_opener.frame(self, window_rect, view_events.items),
                .BufferSearcher => |buffer_searcher| buffer_searcher.frame(self, window_rect, view_events.items),
                .ProjectSearcher => |project_searcher| project_searcher.frame(self, window_rect, view_events.items),
                .Launcher => |launcher| launcher.frame(self, window_rect, view_events.items),
            }
        } else {
            const message = "focus";
            const rect = Rect{
                .x = window_rect.x + max(0, @divTrunc(window_rect.w - (@intCast(Coord, message.len) * self.app.atlas.char_width), 2)),
                .y = window_rect.y + max(0, @divTrunc(window_rect.h - self.app.atlas.char_height, 2)),
                .w = min(window_rect.w, @intCast(Coord, message.len) * self.app.atlas.char_width),
                .h = min(window_rect.h, self.app.atlas.char_height),
            };
            self.queueText(rect, style.text_color, message);
        }

        // set window title
        var window_title: [*c]const u8 = "";
        if (self.getTopViewFilename()) |filename| {
            window_title = std.mem.dupeZ(self.app.frame_allocator, u8, filename) catch oom();
        }
        c.SDL_SetWindowTitle(self.sdl_window, window_title);

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

        // clean up
        self.deinitPoppedViews();
        if (self.close_after_frame) {
            if (self.getTopViewIfEditor()) |editor| editor.save(.Auto);
            if (self.client_address_o) |client_address|
                focus.sendReply(self.app.server_socket, client_address, 0);
            self.deinit();
            self.app.deregisterWindow(self);
        }
    }

    pub fn queueQuad(self: *Window, dst: Rect, src: Rect, color: Color) void {
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

    pub fn pushView(self: *Window, view_ptr: anytype) void {
        if (self.getTopViewIfEditor()) |editor| editor.save(.Auto);
        const tag_name = @typeName(@typeInfo(@TypeOf(view_ptr)).Pointer.child);
        const view = @unionInit(View, tag_name, view_ptr);
        self.views.append(view) catch oom();
    }

    pub fn popView(self: *Window) void {
        if (self.views.items.len > 0) {
            const view = self.views.pop();
            // can't clean up view right away because we might still be inside it's frame function
            self.popped_views.append(view) catch oom();
        }
    }

    fn deinitPoppedViews(self: *Window) void {
        while (self.popped_views.items.len > 0) {
            const view = self.popped_views.pop();
            switch (view) {
                .Editor => |editor| editor.save(.Auto),
                else => {},
            }
            inline for (@typeInfo(ViewTag).Enum.fields) |field| {
                if (@enumToInt(std.meta.activeTag(view)) == field.value) {
                    var view_ptr = @field(view, field.name);
                    view_ptr.deinit();
                }
            }
        }
    }

    // drawing api

    pub fn queueRect(self: *Window, rect: Rect, color: Color) void {
        self.queueQuad(rect, self.app.atlas.white_rect, color);
    }

    pub fn queueText(self: *Window, rect: Rect, color: Color, chars: []const u8) void {
        const max_x = rect.x + rect.w;
        const max_y = rect.y + rect.h;
        var dst: Rect = .{ .x = rect.x, .y = rect.y, .w = 0, .h = 0 };
        for (chars) |char| {
            var src = if (char < self.app.atlas.char_to_rect.len)
                self.app.atlas.char_to_rect[char]
            else
            // TODO tofu
                self.app.atlas.white_rect;
            const max_w = max(0, max_x - dst.x);
            const max_h = max(0, max_y - dst.y);
            const ratio_w = @intToFloat(f64, min(max_w, self.app.atlas.char_width)) / @intToFloat(f64, self.app.atlas.char_width);
            const ratio_h = @intToFloat(f64, min(max_h, self.app.atlas.char_height)) / @intToFloat(f64, self.app.atlas.char_height);
            src.w = @floatToInt(Coord, @floor(@intToFloat(f64, src.w) * ratio_w));
            src.h = @floatToInt(Coord, @floor(@intToFloat(f64, src.h) * ratio_h));
            dst.w = src.w;
            dst.h = src.h;
            self.queueQuad(dst, src, color);
            dst.x += self.app.atlas.char_width;
        }
    }

    // util

    pub const SearcherLayout = struct {
        selector: Rect,
        input: Rect,
    };

    pub fn layoutSearcher(self: *Window, rect: Rect) SearcherLayout {
        const border_thickness = @divTrunc(self.app.atlas.char_height, 8);
        var all_rect = rect;
        const input_rect = all_rect.splitTop(self.app.atlas.char_height, 0);
        const border_rect = all_rect.splitTop(border_thickness, 0);
        const selector_rect = all_rect;
        self.queueRect(border_rect, style.text_color);
        return .{ .selector = selector_rect, .input = input_rect };
    }

    pub const SearcherWithPreviewLayout = struct {
        preview: Rect,
        selector: Rect,
        input: Rect,
    };

    pub fn layoutSearcherWithPreview(self: *Window, rect: Rect) SearcherWithPreviewLayout {
        const border_thickness = @divTrunc(self.app.atlas.char_height, 8);
        var all_rect = rect;
        const preview_rect = all_rect.splitTop(@divTrunc(max(0, rect.h - self.app.atlas.char_height - 2 * border_thickness), 2), 0);
        const border_rect = all_rect.splitTop(border_thickness, 0);
        const searcher_layout = self.layoutSearcher(all_rect);
        self.queueRect(border_rect, style.text_color);
        return .{ .preview = preview_rect, .selector = searcher_layout.selector, .input = searcher_layout.input };
    }
};
