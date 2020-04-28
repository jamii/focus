pub const c = @cImport({
    @cInclude("SDL2/SDL.h");
    @cInclude("SDL2/SDL_opengl.h");
});

pub const std = @import("std");
pub const warn = std.debug.warn;
pub const assert = std.debug.assert;
pub const panic = std.debug.panic;
pub const max = std.math.max;
pub const min = std.math.min;
pub const Allocator = std.mem.Allocator;
pub const ArenaAllocator = std.heap.ArenaAllocator;
pub const ArrayList = std.ArrayList;
pub const HashMap = std.HashMap;
pub const AutoHashMap = std.AutoHashMap;

pub fn DeepHashMap(comptime K: type, comptime V: type) type {
    return HashMap(K, V, std.hash_map.getAutoHashStratFn(K, .DeepRecursive), struct {
        fn eql(a: K, b: K) bool {
            return deepEqual(K, a, b);
        }
    }.eql);
}

pub const str = []const u8;

pub fn deepEqual(comptime T: type, a: T, b: T) bool {
    const ti = @typeInfo(T);
    switch (ti) {
        .Struct => |sti| {
            inline for (sti.fields) |fti| {
                if (!deepEqual(fti.field_type, @field(a, fti.name), @field(b, fti.name))) {
                    return false;
                }
            }
            return true;
        },
        .Array => |ati| {
            for (a) |a_elem, a_ix| {
                if (!deepEqual(pti.child, a_elem, b[a_ix])) {
                    return false;
                }
            }
            return true;
        },
        .Pointer => |pti| {
            switch (pti.size) {
                .One => {
                    return deepEqual(pti.child, a.*, b.*);
                },
                .Many => {
                    comptime {
                        panic("Can't deepEqual {}", T);
                    }
                },
                .Slice => {
                    if (a.len != b.len) {
                        return false;
                    }
                    for (a) |a_elem, a_ix| {
                        if (!deepEqual(pti.child, a_elem, b[a_ix])) {
                            return false;
                        }
                    }
                    return true;
                },
                .C => {
                    comptime {
                        panic("Can't deepEqual {}", T);
                    }
                },
            }
        },
        .Enum => |_| {
            return a == b;
        },
        .Union => |uti| {
            if (uti.tag_type) |tag_type| {
                inline for (uti.fields) |fti| {
                    if (@enumToInt(@as(tag_type, a)) == fti.enum_field.?.value) {
                        if (@enumToInt(@as(tag_type, b)) == fti.enum_field.?.value) {
                            return deepEqual(
                                fti.field_type,
                                @field(a, fti.name),
                                @field(b, fti.name),
                            );
                        } else {
                            return false;
                        }
                    }
                }
                unreachable;
            } else {
                panic("Can't deepEqual {}", T);
            }
        },
        .Int, .Float, .Bool => {
            return a == b;
        },
        else => {
            comptime {
                panic("Can't deepEqual {}", T);
            }
        },
    }
}

fn dumpInner(out_stream: var, indent: u32, thing: var) !void {
    const ti = @typeInfo(@TypeOf(thing));
    switch (ti) {
        .Struct => |sti| {
            try out_stream.writeAll(@typeName(@TypeOf(thing)));
            try out_stream.writeAll("{\n");
            inline for (sti.fields) |field| {
                try out_stream.writeByteNTimes(' ', indent + 4);
                try std.fmt.format(out_stream, ".{} = ", .{field.name});
                try dumpInner(out_stream, indent + 4, @field(thing, field.name));
                try out_stream.writeAll(",\n");
            }
            try out_stream.writeByteNTimes(' ', indent);
            try out_stream.writeAll("}");
        },
        .Array => |ati| {
            if (ati.child == u8) {
                try std.fmt.format(out_stream, "\"{s}\"", .{thing});
            } else {
                try std.fmt.format(out_stream, "[{}]{}[\n", .{ ati.len, @typeName(ati.child) });
                for (thing) |elem| {
                    try out_stream.writeByteNTimes(' ', indent + 4);
                    try dumpInner(out_stream, indent + 4, elem);
                    try out_stream.writeAll(",\n");
                }
                try out_stream.writeByteNTimes(' ', indent);
                try out_stream.writeAll("]");
            }
        },
        .Pointer => |pti| {
            switch (pti.size) {
                .One => {
                    // TODO print '&'
                    try dumpInner(out_stream, indent, thing.*);
                },
                .Many => {
                    // bail
                    try std.fmt.format(out_stream, "{}", .{thing});
                },
                .Slice => {
                    if (pti.child == u8) {
                        try std.fmt.format(out_stream, "\"{s}\"", .{thing});
                    } else {
                        try std.fmt.format(out_stream, "[]{}[\n", .{@typeName(pti.child)});
                        for (thing) |elem| {
                            try out_stream.writeByteNTimes(' ', indent + 4);
                            try dumpInner(out_stream, indent + 4, elem);
                            try out_stream.writeAll(",\n");
                        }
                        try out_stream.writeByteNTimes(' ', indent);
                        try out_stream.writeAll("]");
                    }
                },
                .C => {
                    // bail
                    try std.fmt.format(out_stream, "{}", .{thing});
                },
            }
        },
        else => {
            // bail
            try std.fmt.format(out_stream, "{}", .{thing});
        },
    }
}

pub fn dump(thing: var) void {
    const held = std.debug.getStderrMutex().acquire();
    defer held.release();
    const my_stderr = std.debug.getStderrStream();
    dumpInner(my_stderr.*, 0, thing) catch return;
    my_stderr.writeAll("\n") catch return;
}

pub fn format(allocator: *Allocator, comptime fmt: str, args: var) !str {
    var buf = ArrayList(u8).init(allocator);
    var out = buf.outStream();
    try std.fmt.format(out, fmt, args);
    return buf.items;
}

pub fn subSaturating(comptime T: type, a: T, b: T) T {
    if (b > a) {
        return 0;
    } else {
        return a - b;
    }
}
