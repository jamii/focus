const std = @import("std");
const focus = @import("../../focus.zig");
const generic = @import("./generic.zig");
const u = focus.util;

pub const State = struct {
    allocator: u.Allocator,
    generic: generic.State,

    pub fn init(allocator: u.Allocator, source: []const u8) State {
        return .{
            .allocator = allocator,
            .generic = generic.State.init(allocator, "//", source),
        };
    }

    pub fn deinit(self: *State) void {
        self.* = undefined;
    }

    pub fn updateBeforeChange(self: *State, source: []const u8, delete_range: [2]usize) void {
        self.generic.updateBeforeChange(source, delete_range);
    }

    pub fn updateAfterChange(self: *State, source: []const u8, insert_range: [2]usize) void {
        self.generic.updateAfterChange(source, insert_range);
    }

    pub fn toggleMode(self: *State) void {
        self.generic.toggleMode();
    }

    pub fn highlight(self: State, source: []const u8, range: [2]usize, colors: []u.Color) void {
        self.generic.highlight(source, range, colors);
    }

    pub fn format(self: State, source: []const u8) ?[]const u8 {
        var child_process = std.ChildProcess.init(
            &[_][]const u8{ "setsid", "deno", "fmt", "-" },
            self.allocator,
        );

        child_process.stdin_behavior = .Pipe;
        child_process.stdout_behavior = .Pipe;
        child_process.stderr_behavior = .Pipe;

        child_process.spawn() catch |err|
            u.panic("Error spawning `deno fmt`: {}", .{err});

        child_process.stdin.?.writeAll(source) catch |err|
            u.panic("Error writing to `deno fmt` stdin: {}", .{err});
        child_process.stdin.?.close();
        child_process.stdin = null;

        var stdout = u.ArrayList(u8).init(self.allocator);
        defer stdout.deinit();

        var stderr = u.ArrayList(u8).init(self.allocator);
        defer stderr.deinit();

        child_process.collectOutput(&stdout, &stderr, std.math.maxInt(usize)) catch |err|
            u.panic("Error collecting output from `deno fmt`: {}", .{err});

        const result = child_process.wait() catch |err|
            u.panic("Error waiting for `deno fmt`: {}", .{err});

        if (u.deepEqual(result, .{ .Exited = 0 })) {
            return stdout.toOwnedSlice() catch u.oom();
        } else {
            u.warn("`deno fmt` failed: {s}", .{stderr.items});
            return null;
        }
    }

    pub fn getAddedIndent(self: State, token_ix: usize) usize {
        return self.generic.getAddedIndent(token_ix);
    }
};
