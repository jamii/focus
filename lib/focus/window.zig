const std = @import("std");
const focus = @import("../focus.zig");
const u = focus.util;
const c = focus.util.c;
const Atlas = focus.Atlas;
const App = focus.App;
const Editor = focus.Editor;
const FileOpener = focus.FileOpener;
const ProjectFileOpener = focus.ProjectFileOpener;
const BufferOpener = focus.BufferOpener;
const BufferSearcher = focus.BufferSearcher;
const ProjectSearcher = focus.ProjectSearcher;
const Launcher = focus.Launcher;
const Maker = focus.Maker;
const ErrorLister = focus.ErrorLister;
const style = focus.style;
const mach_compat = focus.mach_compat;

pub const View = union(enum) {
    Editor: *Editor,
    FileOpener: *FileOpener,
    ProjectFileOpener: *ProjectFileOpener,
    BufferOpener: *BufferOpener,
    BufferSearcher: *BufferSearcher,
    ProjectSearcher: *ProjectSearcher,
    Launcher: *Launcher,
    Maker: *Maker,
    ErrorLister: *ErrorLister,
};

pub const Window = struct {
    app: *App,
    // views are allowed to have pointers to previous views on the stack
    views: u.ArrayList(View),
    popped_views: u.ArrayList(View),
    close_after_frame: bool,

    // client socket who opened this window, need to tell them when we close
    client_address_o: ?focus.Address,

    events: *u.ArrayList(mach_compat.Event),
    glfw_window: *c.GLFWwindow,

    texture_buffer: u.ArrayList(u.Quad(u.Vec2f)),
    vertex_buffer: u.ArrayList(u.Quad(u.Vec2f)),
    color_buffer: u.ArrayList(u.Quad(u.Color)),
    index_buffer: u.ArrayList(u32),

    pub fn init(
        app: *App,
        floating: enum { Floating, NotFloating },
    ) Window {
        // pretty arbitrary
        const init_width: usize = 1920;
        const init_height: usize = 1080;

        const events = app.allocator.create(u.ArrayList(mach_compat.Event)) catch u.oom();
        events.* = u.ArrayList(mach_compat.Event).init(app.allocator);

        // init window
        const glfw_window_maybe = c.glfwCreateWindow(init_width, init_height, "focus", null, null);
        const glfw_window = if (glfw_window_maybe) |w| w else {
            u.panic("Error creating glfw window", .{});
        };
        c.glfwSetWindowAttrib(glfw_window, c.GLFW_DECORATED, c.GLFW_FALSE);
        c.glfwSetWindowAttrib(glfw_window, c.GLFW_RESIZABLE, c.GLFW_TRUE);
        c.glfwSetWindowAttrib(glfw_window, c.GLFW_FOCUS_ON_SHOW, c.GLFW_TRUE);
        c.glfwSetWindowAttrib(glfw_window, c.GLFW_FLOATING, switch (floating) {
            .Floating => c.GLFW_TRUE,
            .NotFloating => c.GLFW_FALSE,
        });
        mach_compat.setCallbacks(glfw_window, events);

        // init gl
        c.glfwMakeContextCurrent(glfw_window);
        c.glEnable(c.GL_BLEND);
        c.glBlendFunc(c.GL_SRC_ALPHA, c.GL_ONE_MINUS_SRC_ALPHA);
        c.glDisable(c.GL_CULL_FACE);
        c.glDisable(c.GL_DEPTH_TEST);
        c.glEnable(c.GL_TEXTURE_2D);
        c.glEnableClientState(c.GL_VERTEX_ARRAY);
        c.glEnableClientState(c.GL_TEXTURE_COORD_ARRAY);
        c.glEnableClientState(c.GL_COLOR_ARRAY);

        // disable vsync - too many problems with multiple windows on multiple desktops
        c.glfwSwapInterval(0);

        const self = Window{
            .app = app,
            .views = u.ArrayList(View).init(app.allocator),
            .popped_views = u.ArrayList(View).init(app.allocator),
            .close_after_frame = false,

            .client_address_o = null,

            .events = events,
            .glfw_window = glfw_window,

            .texture_buffer = u.ArrayList(u.Quad(u.Vec2f)).init(app.allocator),
            .vertex_buffer = u.ArrayList(u.Quad(u.Vec2f)).init(app.allocator),
            .color_buffer = u.ArrayList(u.Quad(u.Color)).init(app.allocator),
            .index_buffer = u.ArrayList(u32).init(app.allocator),
        };

        self.loadAtlasTexture(app.atlas);

        return self;
    }

    pub fn loadAtlasTexture(self: Window, atlas: *Atlas) void {
        c.glfwMakeContextCurrent(self.glfw_window);
        {
            var id: u32 = undefined;
            c.glGenTextures(1, &id);
            c.glBindTexture(c.GL_TEXTURE_2D, id);
            c.glTexImage2D(c.GL_TEXTURE_2D, 0, c.GL_ALPHA, atlas.texture_dims.x, atlas.texture_dims.y, 0, c.GL_RGBA, c.GL_UNSIGNED_BYTE, atlas.texture.ptr);
            c.glTexParameteri(c.GL_TEXTURE_2D, c.GL_TEXTURE_MIN_FILTER, c.GL_NEAREST);
            c.glTexParameteri(c.GL_TEXTURE_2D, c.GL_TEXTURE_MAG_FILTER, c.GL_NEAREST);
            u.assert(c.glGetError() == 0);
        }
    }

    pub fn deinit(self: *Window) void {
        self.index_buffer.deinit();
        self.color_buffer.deinit();
        self.vertex_buffer.deinit();
        self.texture_buffer.deinit();

        // TODO do I need to destroy the gl context?
        c.glfwDestroyWindow(self.glfw_window);

        self.events.deinit();
        self.app.allocator.destroy(self.events);

        while (self.views.items.len > 0) {
            const view = self.views.pop().?;
            self.popped_views.append(view) catch u.oom();
        }
        self.deinitPoppedViews();
        self.popped_views.deinit();
        self.views.deinit();
    }

    pub fn getTopView(self: *Window) ?View {
        if (self.views.items.len > 0)
            return self.views.items[self.views.items.len - 1]
        else
            return null;
    }

    pub fn getTopViewIfEditor(self: *Window) ?*Editor {
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

    pub fn frame(self: *Window) void {
        // figure out window size
        var window_width: c_int = 0;
        var window_height: c_int = 0;
        c.glfwGetWindowSize(self.glfw_window, &window_width, &window_height);
        const window_rect = u.Rect{
            .x = 0,
            .y = 0,
            .w = @intCast(window_width),
            .h = @intCast(window_height),
        };

        // handle events
        var view_events = u.ArrayList(mach_compat.Event).init(self.app.frame_allocator);
        {
            const events = self.events.toOwnedSlice() catch u.oom();
            defer self.app.allocator.free(events);
            for (events) |event| {
                var handled = false;
                switch (event) {
                    .key_press, .key_repeat => |key_event| {
                        if (key_event.mods & c.GLFW_MOD_CONTROL != 0) {
                            switch (key_event.key) {
                                c.GLFW_KEY_Q => self.popView(),
                                c.GLFW_KEY_O => {
                                    const init_path = if (self.getTopViewFilename()) |filename|
                                        std.mem.concat(self.app.frame_allocator, u8, &[_][]const u8{ std.fs.path.dirname(filename).?, "/" }) catch u.oom()
                                    else
                                        focus.config.home_path;
                                    const file_opener = FileOpener.init(self.app, init_path);
                                    self.pushView(file_opener);
                                    handled = true;
                                },
                                c.GLFW_KEY_P => {
                                    const project_file_opener = ProjectFileOpener.init(self.app);
                                    self.pushView(project_file_opener);
                                    handled = true;
                                },
                                c.GLFW_KEY_N => {
                                    if (self.getTopViewIfEditor()) |editor| {
                                        const new_window = self.app.registerWindow(Window.init(self.app, .NotFloating));
                                        const new_editor = Editor.init(self.app, editor.buffer, .{});
                                        new_editor.top_pixel = editor.top_pixel;
                                        new_window.pushView(new_editor);
                                    }
                                    handled = true;
                                },
                                c.GLFW_KEY_MINUS => {
                                    self.app.changeFontSize(-1);
                                    handled = true;
                                },
                                c.GLFW_KEY_EQUAL => {
                                    self.app.changeFontSize(1);
                                    handled = true;
                                },
                                c.GLFW_KEY_M => {
                                    const maker = Maker.init(self.app);
                                    self.pushView(maker);
                                    handled = true;
                                },
                                else => {},
                            }
                        }
                        if (key_event.mods & c.GLFW_MOD_ALT != 0) {
                            switch (key_event.key) {
                                c.GLFW_KEY_F => {
                                    const project_dir = if (self.getTopViewIfEditor()) |editor|
                                        editor.buffer.getProjectDir()
                                    else
                                        null;
                                    const project_searcher = ProjectSearcher.init(
                                        self.app,
                                        project_dir orelse focus.config.home_path,
                                        .FixedStrings,
                                        null,
                                    );
                                    self.pushView(project_searcher);
                                    handled = true;
                                },
                                c.GLFW_KEY_P => {
                                    const ignore_buffer = if (self.getTopViewIfEditor()) |editor|
                                        editor.buffer
                                    else
                                        null;
                                    const buffer_opener = BufferOpener.init(self.app, ignore_buffer);
                                    self.pushView(buffer_opener);
                                    handled = true;
                                },
                                c.GLFW_KEY_M => {
                                    const error_lister = ErrorLister.init(self.app);
                                    self.pushView(error_lister);
                                    handled = true;
                                },
                                else => {},
                            }
                        }
                    },
                    .focus_lost => {
                        if (self.getTopViewIfEditor()) |editor| {
                            editor.save(.Auto);
                            editor.buffer.last_lost_focus_ms = self.app.frame_time_ms;
                        }
                        handled = true;
                    },
                    .window_closed => {
                        self.close_after_frame = true;
                        handled = true;
                    },
                    else => {},
                }
                // delegate other events to view
                if (!handled) view_events.append(event) catch u.oom();
            }
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
                .Maker => |maker| maker.frame(self, window_rect, view_events.items),
                .ErrorLister => |error_lister| error_lister.frame(self, window_rect, view_events.items),
            }
        } else {
            const message = "focus";
            const rect = u.Rect{
                .x = window_rect.x + @max(0, @divTrunc(window_rect.w - (@as(u.Coord, @intCast(message.len)) * self.app.atlas.char_width), 2)),
                .y = window_rect.y + @max(0, @divTrunc(window_rect.h - self.app.atlas.char_height, 2)),
                .w = @min(window_rect.w, @as(u.Coord, @intCast(message.len)) * self.app.atlas.char_width),
                .h = @min(window_rect.h, self.app.atlas.char_height),
            };
            self.queueText(rect, style.text_color, message);
        }

        // set window title
        var window_title: [*c]const u8 = "";
        if (self.getTopViewFilename()) |filename| {
            window_title = self.app.frame_allocator.dupeZ(u8, filename) catch u.oom();
        }
        c.glfwSetWindowTitle(self.glfw_window, window_title);

        // render
        c.glfwMakeContextCurrent(self.glfw_window);
        c.glClearColor(0, 0, 0, 1);
        c.glClear(c.GL_COLOR_BUFFER_BIT);
        c.glViewport(
            0,
            0,
            @intCast(window_width),
            @intCast(window_height),
        );
        c.glMatrixMode(c.GL_PROJECTION);
        c.glPushMatrix();
        c.glLoadIdentity();
        c.glOrtho(0.0, @floatFromInt(window_width), @floatFromInt(window_height), 0.0, -1.0, 1.0);
        c.glMatrixMode(c.GL_MODELVIEW);
        c.glPushMatrix();
        c.glLoadIdentity();
        c.glTexCoordPointer(2, c.GL_FLOAT, 0, self.texture_buffer.items.ptr);
        c.glVertexPointer(2, c.GL_FLOAT, 0, self.vertex_buffer.items.ptr);
        c.glColorPointer(4, c.GL_UNSIGNED_BYTE, 0, self.color_buffer.items.ptr);
        c.glDrawElements(c.GL_TRIANGLES, @intCast(self.index_buffer.items.len), c.GL_UNSIGNED_INT, self.index_buffer.items.ptr);
        c.glMatrixMode(c.GL_MODELVIEW);
        c.glPopMatrix();
        c.glMatrixMode(c.GL_PROJECTION);
        c.glPopMatrix();
        c.glfwSwapBuffers(self.glfw_window);

        // reset
        self.texture_buffer.resize(0) catch u.oom();
        self.vertex_buffer.resize(0) catch u.oom();
        self.color_buffer.resize(0) catch u.oom();
        self.index_buffer.resize(0) catch u.oom();

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

    pub fn queueQuad(self: *Window, dst: u.Rect, src: u.Rect, color: u.Color) void {
        const tx = @as(f32, @floatFromInt(src.x)) / @as(f32, @floatFromInt(self.app.atlas.texture_dims.x));
        const ty = @as(f32, @floatFromInt(src.y)) / @as(f32, @floatFromInt(self.app.atlas.texture_dims.y));
        const tw = @as(f32, @floatFromInt(src.w)) / @as(f32, @floatFromInt(self.app.atlas.texture_dims.x));
        const th = @as(f32, @floatFromInt(src.h)) / @as(f32, @floatFromInt(self.app.atlas.texture_dims.y));
        self.texture_buffer.append(.{
            .tl = .{ .x = tx, .y = ty },
            .tr = .{ .x = tx + tw, .y = ty },
            .bl = .{ .x = tx, .y = ty + th },
            .br = .{ .x = tx + tw, .y = ty + th },
        }) catch u.oom();

        const vx = @as(f32, @floatFromInt(dst.x));
        const vy = @as(f32, @floatFromInt(dst.y));
        const vw = @as(f32, @floatFromInt(dst.w));
        const vh = @as(f32, @floatFromInt(dst.h));
        const vertex_ix = @as(u32, @intCast(self.vertex_buffer.items.len * 4));
        self.vertex_buffer.append(.{
            .tl = .{ .x = vx, .y = vy },
            .tr = .{ .x = vx + vw, .y = vy },
            .bl = .{ .x = vx, .y = vy + vh },
            .br = .{ .x = vx + vw, .y = vy + vh },
        }) catch u.oom();

        self.color_buffer.append(.{
            .tl = color,
            .tr = color,
            .bl = color,
            .br = color,
        }) catch u.oom();

        self.index_buffer.appendSlice(&.{
            vertex_ix + 0,
            vertex_ix + 1,
            vertex_ix + 2,

            vertex_ix + 2,
            vertex_ix + 3,
            vertex_ix + 1,
        }) catch u.oom();
    }

    pub fn handleAfterSave(self: *Window) void {
        if (self.getTopView()) |view|
            if (view == .Maker)
                view.Maker.handleAfterSave();
    }

    // view api

    pub fn pushView(self: *Window, view_ptr: anytype) void {
        if (self.getTopViewIfEditor()) |editor| {
            editor.save(.Auto);
            editor.buffer.last_lost_focus_ms = self.app.frame_time_ms;
        }
        const tag_name = comptime tag_name: {
            // TODO this is gross
            const view_type_name = @typeName(@typeInfo(@TypeOf(view_ptr)).pointer.child);
            var iter = std.mem.splitScalar(u8, view_type_name, '.');
            var last_part: ?[]const u8 = null;
            while (iter.next()) |part| last_part = part;
            break :tag_name last_part.?;
        };
        const view = @unionInit(View, tag_name, view_ptr);
        self.views.append(view) catch u.oom();
    }

    pub fn popView(self: *Window) void {
        if (self.getTopViewIfEditor()) |editor| {
            editor.save(.Auto);
            editor.buffer.last_lost_focus_ms = self.app.frame_time_ms;
        }
        if (self.views.items.len > 0) {
            const view = self.views.pop().?;
            // can't clean up view right away because we might still be inside it's frame function
            self.popped_views.append(view) catch u.oom();
        }
    }

    fn deinitPoppedViews(self: *Window) void {
        while (self.popped_views.items.len > 0) {
            const view = self.popped_views.pop().?;
            switch (view) {
                .Editor => |editor| editor.save(.Auto),
                else => {},
            }
            inline for (@typeInfo(@typeInfo(View).@"union".tag_type.?).@"enum".fields) |field| {
                if (@intFromEnum(std.meta.activeTag(view)) == field.value) {
                    var view_ptr = @field(view, field.name);
                    view_ptr.deinit();
                }
            }
        }
    }

    // drawing api

    pub fn queueRect(self: *Window, rect: u.Rect, color: u.Color) void {
        self.queueQuad(rect, self.app.atlas.white_rect, color);
    }

    pub fn queueText(self: *Window, rect: u.Rect, color: u.Color, chars: []const u8) void {
        const max_x = rect.x + rect.w;
        const max_y = rect.y + rect.h;
        var dst: u.Rect = .{ .x = rect.x, .y = rect.y, .w = 0, .h = 0 };
        for (chars) |char| {
            var src = if (char < self.app.atlas.char_to_rect.len)
                self.app.atlas.char_to_rect[char]
            else
                // tofu
                self.app.atlas.char_to_rect[0];
            const max_w = @max(0, max_x - dst.x);
            const max_h = @max(0, max_y - dst.y);
            const ratio_w = @as(f64, @floatFromInt(@min(max_w, self.app.atlas.char_width))) / @as(f64, @floatFromInt(self.app.atlas.char_width));
            const ratio_h = @as(f64, @floatFromInt(@min(max_h, self.app.atlas.char_height))) / @as(f64, @floatFromInt(self.app.atlas.char_height));
            src.w = @intFromFloat(@floor(@as(f64, @floatFromInt(src.w)) * ratio_w));
            src.h = @intFromFloat(@floor(@as(f64, @floatFromInt(src.h)) * ratio_h));
            dst.w = src.w;
            dst.h = src.h;
            self.queueQuad(dst, src, color);
            dst.x += self.app.atlas.char_width;
        }
    }

    // util

    pub const SearcherLayout = struct {
        selector: u.Rect,
        input: u.Rect,
    };

    pub fn layoutSearcher(self: *Window, rect: u.Rect) SearcherLayout {
        const border_thickness = @divTrunc(self.app.atlas.char_height, 8);
        var all_rect = rect;
        const input_rect = all_rect.splitTop(self.app.atlas.char_height, 0);
        const border_rect = all_rect.splitTop(border_thickness, 0);
        const selector_rect = all_rect;
        self.queueRect(border_rect, style.text_color);
        return .{ .selector = selector_rect, .input = input_rect };
    }

    pub const SearcherWithPreviewLayout = struct {
        preview: u.Rect,
        selector: u.Rect,
        input: u.Rect,
    };

    pub fn layoutSearcherWithPreview(self: *Window, rect: u.Rect) SearcherWithPreviewLayout {
        const border_thickness = @divTrunc(self.app.atlas.char_height, 8);
        var all_rect = rect;
        const h = @divTrunc(@max(0, rect.h - self.app.atlas.char_height - 2 * border_thickness), 2);
        const preview_rect = all_rect.splitTop(h, 0);
        const border_rect = all_rect.splitTop(border_thickness, 0);
        const searcher_layout = self.layoutSearcher(all_rect);
        self.queueRect(border_rect, style.text_color);
        return .{ .preview = preview_rect, .selector = searcher_layout.selector, .input = searcher_layout.input };
    }

    pub const ListerLayout = struct {
        preview: u.Rect,
        report: u.Rect,
    };

    pub fn layoutLister(self: *Window, rect: u.Rect) ListerLayout {
        const border_thickness = @divTrunc(self.app.atlas.char_height, 8);
        var all_rect = rect;
        const h = @divTrunc(@max(0, rect.h - 2 * border_thickness), 2);
        const preview_rect = all_rect.splitTop(h, 0);
        const border_rect = all_rect.splitTop(border_thickness, 0);
        const report_rect = all_rect;
        self.queueRect(border_rect, style.text_color);
        return .{ .preview = preview_rect, .report = report_rect };
    }
};
