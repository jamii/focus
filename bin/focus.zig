const builtin = @import("builtin");
const std = @import("std");

const focus = @import("../lib/focus.zig");

pub var gpa = if (builtin.mode == .Debug)
    std.heap.GeneralPurposeAllocator(.{
        .never_unmap = false,
    }){}
else
    null;

pub fn main() void {
    const allocator = if (builtin.mode == .Debug) &gpa.allocator else std.heap.c_allocator;

    const args = std.process.argsAlloc(allocator) catch unreachable;
    var request: focus.Request = .CreateEmptyWindow;
    for (args) |c_arg| {
        const arg: []const u8 = c_arg;
        if (focus.meta.deepEqual(arg, "--launcher")) {
            request = .CreateLauncherWindow;
        }
    }
    std.process.argsFree(allocator, args);

    const server_socket = focus.createServerSocket();
    switch (server_socket.state) {
        .Bound => {
            // if we successfully bound the socket then we're the main process
            // TODO daemonize
            focus.run(allocator, server_socket, request);
        },
        .Unbound => {
            // if the socket is in use then there is already a main process
            // TODO block until the window is closed? for eg git calling EDITOR
            const client_socket = focus.createClientSocket();
            focus.sendRequest(client_socket, server_socket, request);
        },
    }
}
