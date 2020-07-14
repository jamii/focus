const focus = @import("../lib/focus.zig");

pub fn main() void {
    focus.run(@import("std").heap.c_allocator);
}
