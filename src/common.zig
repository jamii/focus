pub const c = @cImport({
    @cInclude("SDL2/SDL.h");
    @cInclude("SDL2/SDL_opengl.h");
    @cInclude("microui.h");
});

pub const atlas = @import("./atlas.zig");
pub const draw = @import("./draw.zig");
pub const memory = @import("./memory.zig");

pub const std = @import("std");
pub const warn = std.debug.warn;
pub const assert = std.debug.assert;
pub const Allocator = std.mem.Allocator;
pub const ArenaAllocator = std.heap.ArenaAllocator;
pub const ArrayList = std.ArrayList;
pub const AutoHashMap = std.AutoHashMap;

pub const str = []const u8;

pub fn debug_format(out_stream: var, indent: u32, thing: var) !void {
    const ti = @typeInfo(@TypeOf(thing));
    switch (ti) {
        .Struct => |sti| {
            try out_stream.writeAll(@typeName(@TypeOf(thing)));
            try out_stream.writeAll("{\n");
            inline for (sti.fields) |field| {
                try out_stream.writeByteNTimes(' ', indent + 4);
                try std.fmt.format(out_stream, ".{} = ", .{field.name});
                try debug_format(out_stream, indent + 4, @field(thing, field.name));
                try out_stream.writeAll(",\n");
            }
            try out_stream.writeAll("}\n");
        },
        .Array => |ati| {
            if (ati.child == u8) {
                try std.fmt.format(out_stream, "\"{s}\"", .{thing});
            } else {
                try std.fmt.format(out_stream, "[{}]{}[\n", .{ati.len, @typeName(ati.child)});
                for (thing) |elem| {
                    try out_stream.writeByteNTimes(' ', indent + 4);
                    try debug_format(out_stream, indent + 4, elem);
                    try out_stream.writeAll(",\n");
                }
                try out_stream.writeAll("]\n");
            }
        },
        .Pointer => |pti| {
            switch (pti.size) {
                .One => {
                    // TODO print '&'
                    try debug_format(out_stream, indent, thing.*);
                },
                .Many => {
                    // bail
                    try out_stream.writeByteNTimes(' ', indent);
                    try std.fmt.format(out_stream, "{}", .{thing});
                },
                .Slice => {
                    if (pti.child == u8) {
                        try std.fmt.format(out_stream, "\"{s}\"", .{thing});
                    } else {
                        try std.fmt.format(out_stream, "[]{}[\n", .{@typeName(pti.child)});
                        for (thing) |elem| {
                            try out_stream.writeByteNTimes(' ', indent + 4);
                            try debug_format(out_stream, indent + 4, elem);
                            try out_stream.writeAll(",\n");
                        }
                        try out_stream.writeAll("]\n");
                    }
                },
                .C => {
                    // bail
                    try out_stream.writeByteNTimes(' ', indent);
                    try std.fmt.format(out_stream, "{}", .{thing});
                },
            }
        },
        else => {
            // bail
            try out_stream.writeByteNTimes(' ', indent);
            try std.fmt.format(out_stream, "{}", .{thing});
        },
    }
}

pub fn d(thing: var) void {
    const held = std.debug.getStderrMutex().acquire();
    defer held.release();
    const my_stderr = std.debug.getStderrStream();
    noasync debug_format(my_stderr.*, 0, thing) catch return;
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
