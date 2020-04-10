const std = @import("std");
const warn = std.debug.warn;
const assert = std.debug.assert;

const c_str = [*c]u8;
const c_const_str = [*c]const u8;
pub usingnamespace @cImport({
    // @cInclude("GL/glew.h");
    // @cInclude("GLFW/glfw3.h");

    // @cInclude("nuklear.h");
    @cInclude("nk_main.h");
});

pub fn main() anyerror!void {
    _ = nk_main();
    warn("yo", .{});
}
