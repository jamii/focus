pub const util = @import("./focus/util.zig");
pub const Atlas = @import("./focus/atlas.zig").Atlas;
pub const Buffer = @import("./focus/buffer.zig").Buffer;
pub const LineWrappedBuffer = @import("./focus/line_wrapped_buffer.zig").LineWrappedBuffer;
pub const Editor = @import("./focus/editor.zig").Editor;
pub const SingleLineEditor = @import("./focus/single_line_editor.zig").SingleLineEditor;
pub const Selector = @import("./focus/selector.zig").Selector;
pub const FileOpener = @import("./focus/file_opener.zig").FileOpener;
pub const ProjectFileOpener = @import("./focus/project_file_opener.zig").ProjectFileOpener;
pub const BufferOpener = @import("./focus/buffer_opener.zig").BufferOpener;
pub const BufferSearcher = @import("./focus/buffer_searcher.zig").BufferSearcher;
pub const ProjectSearcher = @import("./focus/project_searcher.zig").ProjectSearcher;
pub const Launcher = @import("./focus/launcher.zig").Launcher;
pub const ImpRepl = @import("./focus/imp_repl.zig").ImpRepl;
pub const Maker = @import("./focus/maker.zig").Maker;
pub const ErrorLister = @import("./focus/error_lister.zig").ErrorLister;
pub const Window = @import("./focus/window.zig").Window;
pub const Language = @import("./focus/language.zig").Language;
pub const ChildProcess = @import("./focus/child_process.zig").ChildProcess;
pub const style = @import("./focus/style.zig");

const std = @import("std");
const u = util;
const c = util.c;

pub const Request = union(enum) {
    CreateEmptyWindow,
    CreateLauncherWindow,
    CreateEditorWindow: []const u8,
};

// TODO this should just be std.net.Address, but it calculates the wrong address length for abstract domain sockets
pub const Address = struct {
    address: std.net.Address,
    address_len: std.os.socklen_t,
};

pub const RequestAndClientAddress = struct {
    request: Request,
    client_address: Address,
};

pub const ServerSocket = struct {
    address: Address,
    socket: std.os.socket_t,
    state: State,

    const State = enum {
        Bound,
        Unbound,
    };
};

pub fn daemonize(log_filename: []const u8) enum { Parent, Child } {
    // https://stackoverflow.com/questions/17954432/creating-a-daemon-in-linux/17955149#17955149
    if (std.os.fork()) |pid| {
        if (pid != 0) return .Parent;
    } else |err| {
        u.panic("Failed to fork: {}", .{err});
    }
    {
        const err = c.setsid();
        if (err < 0) u.panic("Failed to setsid: {}", .{err});
    }
    // TODO
    //c.signal(c.SIGCHLD, std.os.linux.SIG_IGN);
    //c.signal(c.SIGHUP, std.os.linux.SIG_IGN);
    if (std.os.fork()) |pid| {
        if (pid != 0) std.os.exit(0);
    } else |err| {
        u.panic("Failed to fork: {}", .{err});
    }
    if (std.os.linux.chdir("/home/jamie/") < 0)
        u.panic("Failed to chdir", .{});

    // redirect stdout/err to log
    const log = std.fs.cwd().createFile(log_filename, .{ .read = false, .truncate = false }) catch |err| u.panic("Failed to open log file ({s}): {}", .{ log_filename, err });
    log.seekFromEnd(0) catch |err| u.panic("Failed to seek to end of log file: {}", .{err});
    _ = std.os.dup2(log.handle, std.io.getStdOut().handle) catch |err| u.panic("Failed to redirect stdout to log: {}", .{err});
    _ = std.os.dup2(log.handle, std.io.getStdErr().handle) catch |err| u.panic("Failed to redirect stderr to log: {}", .{err});
    std.debug.print("\n\nFocus daemon started at {}\n", .{std.time.timestamp()});

    return .Child;
}

pub fn createServerSocket(socket_path: []const u8) ServerSocket {
    var address = std.net.Address.initUnix(socket_path) catch |err| u.panic("Failed to init unix socket address: {}", .{err});
    // have to get len before setting the null byte or returns wrong address
    const address_len = address.getOsSockLen();
    address.un.path[0] = 0;
    const socket = std.os.socket(std.os.AF.UNIX, std.os.SOCK.DGRAM | std.os.SOCK.CLOEXEC, 0) catch |err| u.panic("Failed to create unix socket: {}", .{err});
    const state: ServerSocket.State = if (std.os.bind(socket, &address.any, address_len))
        .Bound
    else |err| switch (err) {
        error.AddressInUse => .Unbound,
        else => u.panic("Failed to connect to unix socket: {}", .{err}),
    };
    return ServerSocket{
        .address = .{
            .address = address,
            .address_len = address_len,
        },
        .socket = socket,
        .state = state,
    };
}

pub const ClientSocket = struct {
    socket: std.os.socket_t,
};

pub fn createClientSocket() std.os.socket_t {
    // with zero-length name, will get an autogenerated name
    var address = std.net.Address.initUnix("") catch |err| u.panic("Failed to init unix socket address: {}", .{err});
    // TODO address.getOsSockLen() returns the wrong length for autogenerated names
    const address_len = @sizeOf(c_short);
    address.un.path[0] = 0;
    const socket = std.os.socket(std.os.AF.UNIX, std.os.SOCK.DGRAM, 0) catch |err| u.panic("Failed to create unix socket: {}", .{err});
    std.os.bind(socket, &address.any, address_len) catch |err| u.panic("Failed to connect to unix socket: {}", .{err});
    return socket;
}

pub fn sendRequest(client_socket: std.os.socket_t, server_socket: ServerSocket, request: Request) void {
    const message = switch (request) {
        .CreateEmptyWindow => "CreateEmptyWindow",
        .CreateLauncherWindow => "CreateLauncherWindow",
        .CreateEditorWindow => |filename| filename,
    };
    const len = std.os.sendto(client_socket, message, 0, &server_socket.address.address.any, server_socket.address.address_len) catch |err| u.panic("Failed to send request: {}", .{err});
    u.assert(len == message.len);
}

pub fn receiveRequest(buffer: []u8, server_socket: ServerSocket) ?RequestAndClientAddress {
    var client_address: std.os.sockaddr = undefined;
    // TODO have no idea if this is the correct value
    var client_address_len: std.os.socklen_t = @sizeOf(std.os.sockaddr);
    const len = std.os.recvfrom(server_socket.socket, buffer, std.os.MSG.DONTWAIT, &client_address, &client_address_len) catch |err| {
        switch (err) {
            error.WouldBlock => return null,
            else => u.panic("Failed to recv request: {}", .{err}),
        }
    };
    const message = buffer[0..len];
    const request = if (std.mem.eql(u8, message, "CreateEmptyWindow"))
        .CreateEmptyWindow
    else if (std.mem.eql(u8, message, "CreateLauncherWindow"))
        .CreateLauncherWindow
    else
        Request{ .CreateEditorWindow = message };
    return RequestAndClientAddress{
        .request = request,
        .client_address = .{
            .address = .{ .any = client_address },
            .address_len = client_address_len,
        },
    };
}

pub fn sendReply(server_socket: ServerSocket, client_address: Address, exit_code: u8) void {
    _ = std.os.sendto(server_socket.socket, &[1]u8{exit_code}, 0, &client_address.address.any, client_address.address_len) catch |err| u.warn("Failed to send reply: {}\n", .{err});
}

// returns exit code
pub fn waitReply(client_socket: std.os.socket_t) u8 {
    var buffer: [1]u8 = undefined;
    const len = std.os.recv(client_socket, &buffer, 0) catch |err| {
        u.panic("Failed to recv reply: {}", .{err});
    };
    u.assert(len == 1);
    return buffer[0];
}

const ns_per_frame = @divTrunc(1_000_000_000, 60);

pub fn run(allocator: u.Allocator, server_socket: ServerSocket) void {
    var app = App.init(allocator, server_socket);
    var timer = std.time.Timer.start() catch u.panic("Couldn't start timer", .{});
    while (true) {
        _ = timer.lap();
        app.frame();
        const used_ns = timer.read();
        if (used_ns > ns_per_frame) u.warn("Frame took {} ns\n", .{used_ns});
        // TODO can we correct for drift from sleep imprecision?
        if (used_ns < ns_per_frame) std.time.sleep(ns_per_frame - used_ns);
    }
}

pub const App = struct {
    allocator: u.Allocator,
    server_socket: ServerSocket,
    frame_arena: u.ArenaAllocator,
    frame_allocator: u.Allocator,
    atlas: *Atlas,
    // contains only buffers that were created from files
    // other buffers are just floating around but must be deinited by their owning view
    buffers: u.DeepHashMap([]const u8, *Buffer),
    windows: u.ArrayList(*Window),
    frame_time_ms: i64,
    // used for both buffer_searcher and project_searcher
    last_search_filter: []const u8,
    last_project_search_selected: usize,
    last_file_filter: []const u8,
    last_project_file_opener_selected: usize,
    last_buffer_opener_selected: usize,
    last_error_lister_selected: usize,

    pub fn init(allocator: u.Allocator, server_socket: ServerSocket) *App {
        if (c.SDL_Init(c.SDL_INIT_VIDEO) != 0)
            u.panic("SDL init failed: {s}", .{c.SDL_GetError()});

        var atlas = allocator.create(Atlas) catch u.oom();
        atlas.* = Atlas.init(allocator, 16);
        var self = allocator.create(App) catch u.oom();
        self.* = App{
            .allocator = allocator,
            .server_socket = server_socket,
            .frame_arena = u.ArenaAllocator.init(allocator),
            .frame_allocator = undefined,
            .atlas = atlas,
            .buffers = u.DeepHashMap([]const u8, *Buffer).init(allocator),
            .windows = u.ArrayList(*Window).init(allocator),
            .frame_time_ms = 0,
            .last_search_filter = "",
            .last_project_search_selected = 0,
            .last_file_filter = "",
            .last_project_file_opener_selected = 0,
            .last_buffer_opener_selected = 0,
            .last_error_lister_selected = 0,
        };
        self.frame_allocator = self.frame_arena.allocator();

        return self;
    }

    pub fn deinit(self: *App) void {
        self.allocator.free(self.last_file_filter);
        self.allocator.free(self.last_search_filter);

        for (self.windows.items) |window| {
            window.deinit();
            self.allocator.destroy(window);
        }
        self.windows.deinit();

        var buffer_iter = self.buffers.iterator();
        while (buffer_iter.next()) |entry| {
            self.allocator.free(entry.key_ptr.*);
            entry.value_ptr.deinit();
        }
        self.buffers.deinit();

        self.atlas.deinit();
        self.allocator.destroy(self.atlas);

        self.frame_arena.deinit();

        self.allocator.destroy(self);

        if (u.builtin.mode == .Debug) {
            _ = @import("root").gpa.detectLeaks();
        }
    }

    pub fn quit(self: *App) noreturn {
        self.deinit();
        std.os.exit(0);
    }

    pub fn getBufferFromAbsoluteFilename(self: *App, absolute_filename: []const u8) *Buffer {
        if (self.buffers.get(absolute_filename)) |buffer| {
            return buffer;
        } else {
            const buffer = Buffer.initFromAbsoluteFilename(self, .{}, absolute_filename);
            // buffer might be out of date if file was modified elsewhere
            buffer.refresh();
            self.buffers.put(self.dupe(absolute_filename), buffer) catch u.oom();
            return buffer;
        }
    }

    pub fn registerWindow(self: *App, window: Window) *Window {
        var window_ptr = self.allocator.create(Window) catch u.oom();
        window_ptr.* = window;
        self.windows.append(window_ptr) catch u.oom();
        return window_ptr;
    }

    pub fn deregisterWindow(self: *App, window: *Window) void {
        const i = std.mem.indexOfScalar(*Window, self.windows.items, window).?;
        _ = self.windows.swapRemove(i);
        self.allocator.destroy(window);
    }

    pub fn handleRequest(self: *App, request: Request, client_address: Address) void {
        var new_window: *Window = undefined;
        switch (request) {
            .CreateEmptyWindow => {
                new_window = self.registerWindow(Window.init(self, .NotFloating));
                new_window.client_address_o = client_address;
            },
            .CreateLauncherWindow => {
                new_window = self.registerWindow(Window.init(self, .Floating));
                new_window.client_address_o = client_address;
                const launcher = Launcher.init(self);
                new_window.pushView(launcher);
            },
            .CreateEditorWindow => |filename| {
                new_window = self.registerWindow(Window.init(self, .NotFloating));
                new_window.client_address_o = client_address;
                const new_buffer = self.getBufferFromAbsoluteFilename(filename);
                const new_editor = Editor.init(self, new_buffer, .{});
                new_window.pushView(new_editor);
            },
        }
        // TODO this is a hack - it seems like windows can't receive focus until after their first frame?
        // without this, keypresses sometimes get sent to the current window instead of the new window
        new_window.frame(&[0]c.SDL_Event{});
    }

    pub fn frame(self: *App) void {
        self.frame_time_ms = std.time.milliTimestamp();

        // reset arena
        self.frame_arena.deinit();
        self.frame_arena = u.ArenaAllocator.init(self.allocator);

        // check for requests
        var buffer = self.frame_allocator.alloc(u8, 256 * 1024) catch u.oom();
        while (receiveRequest(buffer, self.server_socket)) |request_and_client_address| {
            self.handleRequest(request_and_client_address.request, request_and_client_address.client_address);
        }

        // fetch events
        var events = u.ArrayList(c.SDL_Event).init(self.frame_allocator);
        {
            var event: c.SDL_Event = undefined;
            while (c.SDL_PollEvent(&event) != 0) {
                if (event.type == c.SDL_QUIT) {
                    // ignore - we're a daemon, we can have zero windows if we want to
                } else {
                    events.append(event) catch u.oom();
                }
            }
        }

        // refresh buffers
        var buffer_iter = self.buffers.iterator();
        while (buffer_iter.next()) |entry| {
            entry.value_ptr.*.refresh();
        }

        // run window frames
        // copy window list because it might change during frame
        const current_windows = self.frame_allocator.dupe(*Window, self.windows.items) catch u.oom();
        for (current_windows) |window| {
            var window_events = u.ArrayList(c.SDL_Event).init(self.frame_allocator);
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
                    if (window_id == c.SDL_GetWindowID(window.sdl_window) or
                        // need to react to mouse up even if it happened outside the window
                        event.type == c.SDL_MOUSEBUTTONUP)
                    {
                        window_events.append(event) catch u.oom();
                    }
                }
            }
            window.frame(window_events.items);
        }

        // TODO separate frame from vsync. if vsync takes more than, say, 1s/120 then we must have missed a frame
    }

    pub fn dupe(self: *App, slice: anytype) @TypeOf(slice) {
        return self.allocator.dupe(@typeInfo(@TypeOf(slice)).Pointer.child, slice) catch u.oom();
    }

    pub fn changeFontSize(self: *App, increment: isize) void {
        self.atlas.deinit();
        const new_font_size = @intCast(isize, self.atlas.point_size) + increment;
        if (new_font_size >= 0) {
            self.atlas.* = Atlas.init(self.allocator, @intCast(usize, new_font_size));
            for (self.windows.items) |window| {
                if (c.SDL_GL_MakeCurrent(window.sdl_window, window.gl_context) != 0)
                    u.panic("Switching to GL context failed: {s}", .{c.SDL_GetError()});
                Window.loadAtlasTexture(self.atlas);
            }
        }
    }

    pub fn getCompletions(self: *App, prefix: []const u8) [][]const u8 {
        var results = u.ArrayList([]const u8).init(self.frame_allocator);

        var buffer_iter = self.buffers.iterator();
        while (buffer_iter.next()) |entry| {
            entry.value_ptr.*.getCompletionsInto(prefix, &results);
        }

        std.sort.sort([]const u8, results.items, {}, struct {
            fn lessThan(_: void, a: []const u8, b: []const u8) bool {
                return std.mem.lessThan(u8, a, b);
            }
        }.lessThan);

        var unique_results = u.ArrayList([]const u8).init(self.frame_allocator);
        for (results.items) |result, i| {
            if (i == 0 or !std.mem.eql(u8, result, results.items[i - 1]))
                unique_results.append(result) catch u.oom();
        }

        return unique_results.toOwnedSlice();
    }

    pub fn handleAfterSave(self: *App) void {
        for (self.windows.items) |window| window.handleAfterSave();
    }
};
