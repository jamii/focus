const builtin = @import("builtin");
const std = @import("std");

const focus = @import("../lib/focus.zig");

pub var gpa = if (builtin.mode == .Debug)
    std.heap.GeneralPurposeAllocator(.{
        .never_unmap = false,
    }){}
else
    null;

const Action = union(enum) {
    Angel,
    Request: focus.Request,
};

pub fn main() void {
    const allocator = if (builtin.mode == .Debug) &gpa.allocator else std.heap.c_allocator;

    const args = std.process.argsAlloc(allocator) catch unreachable;
    var be_angel = false;
    var action: Action = .{ .Request = .CreateEmptyWindow };
    for (args[1..]) |c_arg| {
        const arg: []const u8 = c_arg;
        if (std.mem.startsWith(u8, arg, "--")) {
            if (focus.meta.deepEqual(arg, "--angel")) {
                action = .Angel;
            } else if (focus.meta.deepEqual(arg, "--launcher")) {
                action = .{ .Request = .CreateLauncherWindow };
            } else {
                focus.common.panic("Unrecognized arg: {}", .{arg});
            }
        } else {
            const absolute_filename = std.fs.path.resolve(allocator, &[_][]const u8{arg}) catch focus.common.oom();
            action = .{ .Request = .{ .CreateEditorWindow = absolute_filename } };
        }
    }
    std.process.argsFree(allocator, args);

    const server_socket = focus.createServerSocket();

    switch (action) {
        .Angel => {
            // no daemon (we're probably in a debugger)
            if (server_socket.state != .Bound)
                focus.common.panic("Couldn't bind server socket", .{});
            focus.run(allocator, server_socket);
        },
        .Request => |request| {
            // if we successfully bound the socket then we need to create the daemon
            if (server_socket.state == .Bound) {
                if (focus.daemonize() == .Child) {
                    focus.run(allocator, server_socket);
                    // run doesn't return
                    unreachable;
                }
            }

            // ask the main process to do something
            const client_socket = focus.createClientSocket();
            focus.sendRequest(client_socket, server_socket, request);
            switch (request) {
                .CreateEmptyWindow, .CreateLauncherWindow => {},
                .CreateEditorWindow => |filename| allocator.free(filename),
            }

            // wait until it's done
            const exit_code = focus.waitReply(client_socket);
            std.os.exit(exit_code);
        },
    }
}
