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

    pub fn format(self: State, frame_allocator: u.Allocator, source: []const u8) ?[]const u8 {
        var child_process = std.process.Child.init(
            &[_][]const u8{ "setsid", "rustfmt" },
            self.allocator,
        );

        child_process.stdin_behavior = .Pipe;
        child_process.stdout_behavior = .Pipe;
        child_process.stderr_behavior = .Pipe;

        child_process.spawn() catch |err|
            u.panic("Error spawning `rustfmt`: {}", .{err});

        child_process.stdin.?.writeAll(source) catch |err|
            u.panic("Error writing to `rustfmt` stdin: {}", .{err});
        child_process.stdin.?.close();
        child_process.stdin = null;

        var stdout = std.ArrayListUnmanaged(u8).empty;
        var stderr = std.ArrayListUnmanaged(u8).empty;

        child_process.collectOutput(frame_allocator, &stdout, &stderr, std.math.maxInt(usize)) catch |err|
            u.panic("Error collecting output from `rustfmt`: {}", .{err});

        const result = child_process.wait() catch |err|
            u.panic("Error waiting for `rustfmt`: {}", .{err});

        if (u.deepEqual(result, .{ .Exited = 0 })) {
            return stdout.toOwnedSlice(frame_allocator) catch u.oom();
        } else {
            u.warn("`rustfmt` failed: {s}", .{stderr.items});
            return null;
        }
    }

    pub fn getAddedIndent(self: State, token_ix: usize) usize {
        return self.generic.getAddedIndent(token_ix);
    }
};
