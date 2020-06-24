const focus = @import("../lib/focus.zig");

pub fn main() !void {
    try focus.run(@import("std").heap.c_allocator);
}
