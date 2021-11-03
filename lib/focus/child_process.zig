const focus = @import("../focus.zig");
usingnamespace focus.common;
const Buffer = focus.Buffer;

pub const ChildProcess = struct {
    child_process: *std.ChildProcess,

    pub fn init(allocator: *Allocator, dirname: []const u8, args: []const []const u8) ChildProcess {
        const child_process = std.ChildProcess.init(args, allocator) catch |err|
            panic("{} while running args: {s}", .{ err, args });
        child_process.cwd = dirname;
        child_process.stdin_behavior = .Ignore;
        child_process.stdout_behavior = .Pipe;
        child_process.stderr_behavior = .Pipe;
        child_process.spawn() catch |err|
            panic("{} while running args: {s}", .{ err, args });
        for (&[_]std.fs.File{ child_process.stdout.?, child_process.stderr.? }) |file| {
            _ = std.os.fcntl(file.handle, std.os.linux.F_SETFL, std.os.linux.O_NONBLOCK) catch |err|
                panic("Err setting pipe nonblock: {}", .{err});
        }
        return .{ .child_process = child_process };
    }

    pub fn deinit(self: *ChildProcess) void {
        const pgid = std.os.linux.syscall1(.getpgid, @bitCast(usize, @as(isize, self.child_process.pid)));
        std.os.kill(-@intCast(i32, pgid), std.os.SIGKILL) catch {};
        self.child_process.stdout.?.close();
        self.child_process.stderr.?.close();
        self.child_process.deinit();
    }

    pub fn poll(self: *ChildProcess, buffer: *Buffer) usize {
        var num_bytes_read: usize = 0;
        const tmp = buffer.app.frame_allocator.alloc(u8, 1024) catch oom();
        for (&[_]std.fs.File{ self.child_process.stdout.?, self.child_process.stderr.? }) |file| {
            while (true) {
                if (file.read(tmp)) |len| {
                    buffer.insert(buffer.bytes.items.len, tmp[0..len]);
                    num_bytes_read += len;
                    if (len < tmp.len)
                        break;
                } else |err| {
                    switch (err) {
                        error.WouldBlock => break,
                        else => panic("Err reading pipe: {}", .{err}),
                    }
                }
            }
        }
        return num_bytes_read;
    }
};
