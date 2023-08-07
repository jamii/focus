const std = @import("std");
const focus = @import("../focus.zig");
const u = focus.util;
const c = focus.util.c;

pub const ChildProcess = struct {
    child_process: std.ChildProcess,

    pub fn init(allocator: u.Allocator, dirname: []const u8, args: []const []const u8) ChildProcess {
        const full_args = std.mem.concat(allocator, []const u8, &.{
            &.{"setsid"},
            args,
        }) catch u.oom();
        var child_process = std.ChildProcess.init(full_args, allocator);
        child_process.cwd = dirname;
        child_process.stdin_behavior = .Ignore;
        child_process.stdout_behavior = .Pipe;
        child_process.stderr_behavior = .Pipe;
        child_process.spawn() catch |err|
            u.panic("{} while running args: {s}", .{ err, full_args });
        for (&[_]std.fs.File{ child_process.stdout.?, child_process.stderr.? }) |file| {
            _ = std.os.fcntl(file.handle, std.os.linux.F.SETFL, std.os.linux.O.NONBLOCK) catch |err|
                u.panic("Err setting pipe nonblock: {}", .{err});
        }
        return .{ .child_process = child_process };
    }

    pub fn deinit(self: *ChildProcess) void {
        const pgid = std.os.linux.syscall1(.getpgid, @as(usize, @bitCast(@as(isize, self.child_process.id))));
        std.os.kill(-@as(i32, @intCast(pgid)), std.os.SIG.KILL) catch {};
        self.child_process.stdout.?.close();
        self.child_process.stderr.?.close();
    }

    pub fn poll(self: ChildProcess) enum { Running, Finished } {
        const wait = std.os.waitpid(self.child_process.id, std.os.linux.WNOHANG);
        return if (wait.id == self.child_process.id) .Finished else .Running;
    }

    pub fn read(self: ChildProcess, allocator: u.Allocator) []const u8 {
        var bytes = u.ArrayList(u8).initCapacity(allocator, 4096) catch u.oom();
        defer bytes.deinit();
        var start: usize = 0;
        for (&[_]std.fs.File{ self.child_process.stdout.?, self.child_process.stderr.? }) |file| {
            while (true) {
                bytes.expandToCapacity();
                if (file.read(bytes.items[start..])) |num_bytes_read| {
                    start += num_bytes_read;
                    if (num_bytes_read == 0)
                        break;
                } else |err| {
                    switch (err) {
                        error.WouldBlock => break,
                        else => u.panic("Err reading pipe: {}", .{err}),
                    }
                }
                bytes.ensureTotalCapacity(start + 1) catch u.oom();
            }
        }
        bytes.shrinkAndFree(start);
        return bytes.toOwnedSlice() catch u.oom();
    }
};
