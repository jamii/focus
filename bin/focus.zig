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
    focus.run(allocator);
}
