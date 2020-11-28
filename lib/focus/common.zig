const focus = @import("../focus.zig");
const meta = focus.meta;

pub const c = @cImport({
    @cInclude("SDL2/SDL.h");
    @cInclude("SDL2/SDL_syswm.h");
    @cInclude("SDL2/SDL_opengl.h");
    @cInclude("SDL2/SDL_ttf.h");
});

pub const builtin = @import("builtin");
pub const std = @import("std");
pub const warn = std.debug.warn;
pub const assert = std.debug.assert;
pub const max = std.math.max;
pub const min = std.math.min;
pub const Allocator = std.mem.Allocator;
pub const ArenaAllocator = std.heap.ArenaAllocator;
pub const ArrayList = std.ArrayList;
pub const HashMap = std.HashMap;
pub const AutoHashMap = std.AutoHashMap;

// TODO should probably preallocate memory for panic message
pub fn panic(comptime fmt: []const u8, args: anytype) noreturn {
    var buf = ArrayList(u8).init(std.heap.c_allocator);
    var out = buf.outStream();
    const message: []const u8 = message: {
        std.fmt.format(out, fmt, args) catch |err| {
            switch (err) {
                error.OutOfMemory => break :message "OOM inside panic",
            }
        };
        break :message buf.toOwnedSlice();
    };
    @panic(message);
}

pub fn oom() noreturn {
    @panic("Out of memory");
}

pub fn DeepHashMap(comptime K: type, comptime V: type) type {
    return std.HashMap(K, V, struct {
        fn hash(key: K) u64 {
            return meta.deepHash(key);
        }
    }.hash, struct {
        fn equal(a: K, b: K) bool {
            return meta.deepEqual(a, b);
        }
    }.equal, std.hash_map.DefaultMaxLoadPercentage);
}

pub fn dump(thing: anytype) void {
    const held = std.debug.getStderrMutex().acquire();
    defer held.release();
    const my_stderr = std.io.getStdErr().writer();
    dumpInto(my_stderr, 0, thing) catch return;
    my_stderr.writeAll("\n") catch return;
}

pub fn dumpInto(out_stream: anytype, indent: u32, thing: anytype) anyerror!void {
    const ti = @typeInfo(@TypeOf(thing));
    switch (ti) {
        .Pointer => |pti| {
            switch (pti.size) {
                .One => {
                    try out_stream.writeAll("&");
                    try dumpInto(out_stream, indent, thing.*);
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
                            try dumpInto(out_stream, indent + 4, elem);
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
        .Array => |ati| {
            if (ati.child == u8) {
                try std.fmt.format(out_stream, "\"{s}\"", .{thing});
            } else {
                try std.fmt.format(out_stream, "[{}]{}[\n", .{ ati.len, @typeName(ati.child) });
                for (thing) |elem| {
                    try out_stream.writeByteNTimes(' ', indent + 4);
                    try dumpInto(out_stream, indent + 4, elem);
                    try out_stream.writeAll(",\n");
                }
                try out_stream.writeByteNTimes(' ', indent);
                try out_stream.writeAll("]");
            }
        },
        .Struct => |sti| {
            try out_stream.writeAll(@typeName(@TypeOf(thing)));
            try out_stream.writeAll("{\n");
            inline for (sti.fields) |field| {
                try out_stream.writeByteNTimes(' ', indent + 4);
                try std.fmt.format(out_stream, ".{} = ", .{field.name});
                try dumpInto(out_stream, indent + 4, @field(thing, field.name));
                try out_stream.writeAll(",\n");
            }
            try out_stream.writeByteNTimes(' ', indent);
            try out_stream.writeAll("}");
        },
        .Union => |uti| {
            if (uti.tag_type) |tag_type| {
                try out_stream.writeAll(@typeName(@TypeOf(thing)));
                try out_stream.writeAll("{\n");
                inline for (@typeInfo(tag_type).Enum.fields) |fti| {
                    if (@enumToInt(std.meta.activeTag(thing)) == fti.value) {
                        try out_stream.writeByteNTimes(' ', indent + 4);
                        try std.fmt.format(out_stream, ".{} = ", .{fti.name});
                        try dumpInto(out_stream, indent + 4, @field(thing, fti.name));
                        try out_stream.writeAll("\n");
                        try out_stream.writeByteNTimes(' ', indent);
                        try out_stream.writeAll("}");
                    }
                }
            } else {
                // bail
                try std.fmt.format(out_stream, "{}", .{thing});
            }
        },
        else => {
            // bail
            try std.fmt.format(out_stream, "{}", .{thing});
        },
    }
}

pub fn format(allocator: *Allocator, comptime fmt: []const u8, args: anytype) []const u8 {
    var buf = ArrayList(u8).init(allocator);
    var out = buf.outStream();
    std.fmt.format(out, fmt, args) catch oom();
    return buf.items;
}

pub fn subSaturating(a: anytype, b: @TypeOf(a)) @TypeOf(a) {
    if (b > a) {
        return 0;
    } else {
        return a - b;
    }
}

// --------------------------------------------------------------------------------
// drawing stuff

pub const Coord = i32;

pub const Rect = struct {
    x: Coord,
    y: Coord,
    w: Coord,
    h: Coord,

    pub fn shrink(self: *const Rect, margin: Coord) Rect {
        assert(self.w >= 2 * margin);
        assert(self.h >= 2 * margin);
        return Rect{ .x = self.x + margin, .y = self.y + margin, .w = self.w - (2 * margin), .h = self.h - (2 * margin) };
    }

    // TODO split into pointers

    pub fn splitLeft(self: *Rect, w: Coord, margin: Coord) Rect {
        assert(self.w >= w);
        const split = Rect{ .x = self.x, .y = self.y, .w = w, .h = self.h };
        self.x += w + margin;
        self.w -= w + margin;
        return split;
    }

    pub fn splitRight(self: *Rect, w: Coord, margin: Coord) Rect {
        assert(self.w >= w);
        const split = Rect{ .x = self.x + self.w - w, .y = self.y, .w = w, .h = self.h };
        self.w -= w + margin;
        return split;
    }

    pub fn splitBottom(self: *Rect, h: Coord, margin: Coord) Rect {
        assert(self.h >= h);
        const split = Rect{ .x = self.x, .y = self.y + self.h - h, .w = self.w, .h = h };
        self.h -= h + margin;
        return split;
    }

    pub fn splitTop(self: *Rect, h: Coord, margin: Coord) Rect {
        assert(self.h >= h);
        const split = Rect{ .x = self.x, .y = self.y, .w = self.w, .h = h };
        self.y += h + margin;
        self.h -= h + margin;
        return split;
    }

    pub fn contains(self: Rect, x: Coord, y: Coord) bool {
        return x >= self.x and x < (self.x + self.w) and y >= self.y and (y < self.y + self.h);
    }
};

pub const Vec2 = struct {
    x: Coord,
    y: Coord,
};

pub const Color = packed struct {
    r: u8,
    g: u8,
    b: u8,
    a: u8,
};

pub const Vec2f = packed struct {
    x: f32,
    y: f32,
};

pub fn Tri(comptime t: type) type {
    // TODO which direction?
    return packed struct {
        a: t,
        b: t,
        c: t,
    };
}

pub fn Quad(comptime t: type) type {
    return packed struct {
        tl: t,
        tr: t,
        bl: t,
        br: t,
    };
}

// --------------------------------------------------------------------------------
// search stuff

const ScoredItem = struct {
    score: usize,
    item: []const u8,
};
pub fn fuzzy_search(allocator: *Allocator, items: []const []const u8, filter: []const u8) [][]const u8 {
    var scored_items = ArrayList(ScoredItem).init(allocator);
    defer scored_items.deinit();

    for (items) |item| {
        if (filter.len > 0) {
            var score: usize = std.math.maxInt(usize);
            var any_match = false;
            const filter_start_char = filter[0];
            for (item) |start_char, start| {
                if (start_char == filter_start_char) {
                    var is_match = true;
                    var end = start;
                    for (filter[1..]) |char| {
                        if (std.mem.indexOfScalarPos(u8, item, end, char)) |new_end| {
                            end = new_end + 1;
                        } else {
                            is_match = false;
                            break;
                        }
                    }
                    if (is_match) {
                        score = min(score, end - start);
                        any_match = true;
                    }
                }
            }
            if (any_match) scored_items.append(.{ .score = score, .item = item }) catch oom();
        } else {
            const score = 0;
            scored_items.append(.{ .score = score, .item = item }) catch oom();
        }
    }
    std.sort.sort(ScoredItem, scored_items.items, {}, struct {
        fn lessThan(_: void, a: ScoredItem, b: ScoredItem) bool {
            return a.score < b.score;
        }
    }.lessThan);

    var results = ArrayList([]const u8).init(allocator);
    for (scored_items.items) |scored_item| {
        results.append(scored_item.item) catch oom();
    }
    return results.toOwnedSlice();
}
