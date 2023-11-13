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

    fn formatWithTabs(self: State, frame_allocator: u.Allocator, source: []const u8) ?[]const u8 {
        _ = self;

        var child_process = std.ChildProcess.init(
            &[_][]const u8{ "setsid", "gofmt", "-s" },
            frame_allocator,
        );

        child_process.stdin_behavior = .Pipe;
        child_process.stdout_behavior = .Pipe;
        child_process.stderr_behavior = .Pipe;

        child_process.spawn() catch |err|
            u.panic("Error spawning `gofmt`: {}", .{err});

        child_process.stdin.?.writeAll(source) catch |err|
            u.panic("Error writing to `gofmt` stdin: {}", .{err});
        child_process.stdin.?.close();
        child_process.stdin = null;

        var stdout = u.ArrayList(u8).init(frame_allocator);
        var stderr = u.ArrayList(u8).init(frame_allocator);

        child_process.collectOutput(&stdout, &stderr, std.math.maxInt(usize)) catch |err|
            u.panic("Error collecting output from `gofmt`: {}", .{err});

        const result = child_process.wait() catch |err|
            u.panic("Error waiting for `gofmt`: {}", .{err});

        if (u.deepEqual(result, .{ .Exited = 0 })) {
            return stdout.items;
        } else {
            u.warn("`gofmt` failed: {s}", .{stderr.items});
            return null;
        }
    }

    pub fn format(self: State, frame_allocator: u.Allocator, source: []const u8) ?[]const u8 {
        return if (self.formatWithTabs(frame_allocator, source)) |new_source|
            self.afterLoad(frame_allocator, new_source)
        else
            null;
    }

    pub fn getAddedIndent(self: State, token_ix: usize) usize {
        return self.generic.getAddedIndent(token_ix);
    }

    pub fn afterLoad(self: State, frame_allocator: u.Allocator, source: []const u8) []const u8 {
        _ = self;

        var replaced = u.ArrayList(u8).initCapacity(frame_allocator, source.len) catch u.oom();
        for (source) |char| {
            if (char == '\t') {
                replaced.appendNTimes(' ', 4) catch u.oom();
            } else {
                replaced.append(char) catch u.oom();
            }
        }
        return replaced.items;
    }

    pub fn beforeSave(self: State, frame_allocator: u.Allocator, source: []const u8) []const u8 {
        return self.formatWithTabs(frame_allocator, source) orelse source;
    }
};
