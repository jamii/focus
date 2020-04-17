pub usingnamespace @cImport({
    @cInclude("SDL2/SDL.h");
    @cInclude("SDL2/SDL_opengl.h");
    @cInclude("microui.h");
});

pub const atlas = @import("./atlas.zig");
pub const draw = @import("./draw.zig");

pub const std = @import("std");
pub const warn = std.debug.warn;
pub const assert = std.debug.assert;
pub const Allocator = std.mem.Allocator;

pub fn d(thing: var) void {
    warn("{}\n", .{thing});
}

pub fn zero(comptime t: type) t {
    var value: t = undefined;
    const ti = @typeInfo(t);
    switch (ti) {
        .Int, .Float => {
            value = 0;
        },
        .Array => |ati| {
            var i: usize = 0;
            while (i < ati.len) : (i += 1) {
                value[i] = zero(ati.child);
            }
        },
        .Struct => |sti| {
            inline for (sti.fields) |field| {
                @field(value, field.name) = zero(field.field_type);
            }
        },
        else => panic(),
    }
    return value;
}

pub const Vec2 = packed struct {
    x: u32,
    y: u32,
};

pub const Rect = packed struct {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
};

pub const Color = packed struct {
    r: u8,
    g: u8,
    b: u8,
    a: u8,
};
