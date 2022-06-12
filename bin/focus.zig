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
    const allocator = if (builtin.mode == .Debug) gpa.allocator() else std.heap.c_allocator;
    var arena = focus.util.ArenaAllocator.init(allocator);

    const args = std.process.argsAlloc(arena.allocator()) catch focus.util.oom();
    var action: Action = .{ .Request = .CreateEmptyWindow };
    var arg_ix: usize = 1;
    while (arg_ix < args.len) : (arg_ix += 1) {
        const arg: []const u8 = args[arg_ix];
        if (std.mem.startsWith(u8, arg, "--")) {
            if (focus.util.deepEqual(arg, "--angel")) {
                action = .Angel;
            } else if (focus.util.deepEqual(arg, "--launcher")) {
                action = .{ .Request = .CreateLauncherWindow };
            } else if (focus.util.deepEqual(arg, "--maker")) {
                arg_ix += 1;
                const dirname: []const u8 = args[arg_ix];
                arg_ix += 1;
                const command: []const u8 = args[arg_ix];
                action = .{ .Request = .{ .CreateMakerWindow = .{ .dirname = dirname, .command = command } } };
            } else {
                focus.util.panic("Unrecognized arg: {s}", .{arg});
            }
        } else {
            const absolute_filename = std.fs.path.resolve(arena.allocator(), &[_][]const u8{arg}) catch focus.util.oom();
            action = .{ .Request = .{ .CreateEditorWindow = absolute_filename } };
        }
    }

    const socket_path = focus.util.format(arena.allocator(), "#{s}", .{args[0]});
    const server_socket = focus.createServerSocket(socket_path);

    switch (action) {
        .Angel => {
            // no daemon (we're probably in a debugger)
            if (server_socket.state != .Bound)
                focus.util.panic("Couldn't bind server socket", .{});
            focus.run(allocator, server_socket);
        },
        .Request => |request| {
            // if we successfully bound the socket then we need to create the daemon
            if (server_socket.state == .Bound) {
                const log_filename = focus.util.format(arena.allocator(), "/home/jamie/.log/{s}.log", .{std.fs.path.basename(args[0])});
                if (focus.daemonize(log_filename) == .Child) {
                    focus.run(allocator, server_socket);
                    // run doesn't return
                    unreachable;
                }
            }

            // ask the main process to do something
            const client_socket = focus.createClientSocket();
            focus.sendRequest(allocator, client_socket, server_socket, request);

            // wait until it's done
            const exit_code = focus.waitReply(client_socket);
            arena.deinit();
            std.os.exit(exit_code);
        },
    }
}
