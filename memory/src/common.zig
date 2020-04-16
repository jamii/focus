pub usingnamespace @cImport({
    @cInclude("SDL2/SDL.h");
    @cInclude("SDL2/SDL_opengl.h");
    @cInclude("microui.h");
});

pub const std = @import("std");
pub const warn = std.debug.warn;
pub const assert = std.debug.assert;
pub const Allocator = std.mem.Allocator;

pub fn d(thing: var) void {
    warn("{}\n", .{thing});
}
