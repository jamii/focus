pub const common = @import("./focus/common.zig");
pub const meta = @import("./focus/meta.zig");
pub const Atlas = @import("./focus/atlas.zig").Atlas;
pub const Buffer = @import("./focus/buffer.zig").Buffer;
pub const LineWrappedBuffer = @import("./focus/line_wrapped_buffer.zig").LineWrappedBuffer;
pub const Editor = @import("./focus/editor.zig").Editor;
pub const SingleLineEditor = @import("./focus/single_line_editor.zig").SingleLineEditor;
pub const Selector = @import("./focus/selector.zig").Selector;
pub const FileOpener = @import("./focus/file_opener.zig").FileOpener;
pub const ProjectFileOpener = @import("./focus/project_file_opener.zig").ProjectFileOpener;
pub const BufferSearcher = @import("./focus/buffer_searcher.zig").BufferSearcher;
pub const ProjectSearcher = @import("./focus/project_searcher.zig").ProjectSearcher;
pub const Window = @import("./focus/window.zig").Window;
pub const style = @import("./focus/style.zig");

usingnamespace common;

const ns_per_frame = @divTrunc(1_000_000_000, 60);

pub fn run(allocator: *Allocator) void {
    var app = App.init(allocator);
    var timer = std.time.Timer.start() catch panic("Couldn't start timer", .{});
    while (true) {
        _ = timer.lap();
        app.frame();
        const used_ns = timer.read();
        if (used_ns > ns_per_frame) warn("Frame took {} ns\n", .{used_ns});
        // TODO can we correct for drift from sleep imprecision?
        if (used_ns < ns_per_frame) std.time.sleep(ns_per_frame - used_ns);
    }
}

pub const App = struct {
    allocator: *Allocator,
    frame_arena: ArenaAllocator,
    frame_allocator: *Allocator,
    atlas: *Atlas,
    // contains only buffers that were created from files
    // other buffers are just floating around
    buffers: DeepHashMap([]const u8, *Buffer),
    scratch_buffer: *Buffer,
    windows: ArrayList(*Window),
    frame_time_ms: i64,

    pub fn init(allocator: *Allocator) *App {
        if (c.SDL_Init(c.SDL_INIT_EVERYTHING) != 0)
            panic("SDL init failed: {s}", .{c.SDL_GetError()});

        var atlas = allocator.create(Atlas) catch oom();
        atlas.* = Atlas.init(allocator, 16);
        var self = allocator.create(App) catch oom();
        self.* = App{
            .allocator = allocator,
            .frame_arena = ArenaAllocator.init(allocator),
            .frame_allocator = undefined,
            .atlas = atlas,
            .buffers = DeepHashMap([]const u8, *Buffer).init(allocator),
            .scratch_buffer = undefined, // defined below
            .windows = ArrayList(*Window).init(allocator),
            .frame_time_ms = 0,
        };
        self.frame_allocator = &self.frame_arena.allocator;

        self.scratch_buffer = Buffer.initEmpty(self);
        self.scratch_buffer.insert(0, "some initial text\nand some more\nshort\nre" ++ ("a" ** 1000) ++ "lly long" ++ ("abc\n" ** 10));
        const editor = Editor.init(self, self.scratch_buffer, true);
        const window = self.registerWindow(Window.init(self, .{ .Editor = editor }));

        return self;
    }

    pub fn deinit(self: *App) void {
        for (self.windows.items) |window| {
            window.deinit();
            self.allocator.destroy(window);
        }
        self.windows.deinit();

        self.scratch_buffer.deinit();

        var buffer_iter = self.buffers.iterator();
        while (buffer_iter.next()) |kv| {
            self.allocator.free(kv.key);
            kv.value.deinit();
        }
        self.buffers.deinit();

        self.atlas.deinit();
        self.allocator.destroy(self.atlas);

        self.frame_arena.deinit();

        self.allocator.destroy(self);

        if (builtin.mode == .Debug) {
            _ = @import("root").gpa.detectLeaks();
        }
    }

    pub fn getBufferFromAbsoluteFilename(self: *App, absolute_filename: []const u8) *Buffer {
        if (self.buffers.get(absolute_filename)) |buffer| {
            return buffer;
        } else {
            const buffer = Buffer.initFromAbsoluteFilename(self, absolute_filename);
            self.buffers.put(self.dupe(absolute_filename), buffer) catch oom();
            return buffer;
        }
    }

    pub fn registerWindow(self: *App, window: Window) *Window {
        var window_ptr = self.allocator.create(Window) catch oom();
        window_ptr.* = window;
        self.windows.append(window_ptr) catch oom();
        return window_ptr;
    }

    pub fn deregisterWindow(self: *App, window: *Window) void {
        const i = std.mem.indexOfScalar(*Window, self.windows.items, window).?;
        _ = self.windows.swapRemove(i);
        self.allocator.destroy(window);
    }

    pub fn frame(self: *App) void {
        self.frame_time_ms = std.time.milliTimestamp();

        // reset arena
        self.frame_arena.deinit();
        self.frame_arena = ArenaAllocator.init(self.allocator);

        // fetch events
        var events = ArrayList(c.SDL_Event).init(self.frame_allocator);
        {
            var event: c.SDL_Event = undefined;
            while (c.SDL_PollEvent(&event) != 0) {
                if (event.type == c.SDL_QUIT) {
                    self.deinit();
                    std.os.exit(0);
                }
                events.append(event) catch oom();
            }
        }

        // refresh buffers
        var buffer_iter = self.buffers.iterator();
        while (buffer_iter.next()) |kv| {
            kv.value.refresh();
        }

        // run window frames
        if (self.windows.items.len == 0) {
            std.os.exit(0);
        }
        // copy window list because it might change during frame
        const current_windows = std.mem.dupe(self.frame_allocator, *Window, self.windows.items) catch oom();
        for (current_windows) |window| {
            var window_events = ArrayList(c.SDL_Event).init(self.frame_allocator);
            for (events.items) |event| {
                const window_id_o: ?u32 = switch (event.type) {
                    c.SDL_WINDOWEVENT => event.window.windowID,
                    c.SDL_KEYDOWN, c.SDL_KEYUP => event.key.windowID,
                    c.SDL_TEXTEDITING => event.edit.windowID,
                    c.SDL_TEXTINPUT => event.text.windowID,
                    c.SDL_MOUSEBUTTONDOWN, c.SDL_MOUSEBUTTONUP => event.button.windowID,
                    c.SDL_MOUSEWHEEL => event.wheel.windowID,

                    else => null,
                };
                if (window_id_o) |window_id| {
                    if (window_id == c.SDL_GetWindowID(window.sdl_window)) {
                        window_events.append(event) catch oom();
                    }
                }
            }
            window.frame(window_events.items);
        }

        // TODO separate frame from vsync. if vsync takes more than, say, 1s/120 then we must have missed a frame
    }

    pub fn dupe(self: *App, slice: anytype) @TypeOf(slice) {
        return self.allocator.dupe(@typeInfo(@TypeOf(slice)).Pointer.child, slice) catch oom();
    }

    pub fn changeFontSize(self: *App, increment: isize) void {
        self.atlas.deinit();
        const new_font_size = @intCast(isize, self.atlas.point_size) + increment;
        if (new_font_size >= 0) {
            self.atlas.* = Atlas.init(self.allocator, @intCast(usize, new_font_size));
            for (self.windows.items) |window| {
                if (c.SDL_GL_MakeCurrent(window.sdl_window, window.gl_context) != 0)
                    panic("Switching to GL context failed: {s}", .{c.SDL_GetError()});
                Window.loadAtlasTexture(self.atlas);
            }
        }
    }
};
