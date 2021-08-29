const focus = @import("../focus.zig");
const meta = focus.meta;

pub const c = @cImport({
    @cInclude("SDL2/SDL.h");
    @cInclude("SDL2/SDL_syswm.h");
    @cInclude("SDL2/SDL_opengl.h");
    @cInclude("SDL2/SDL_ttf.h");
    @cInclude("unistd.h");
    @cInclude("signal.h");
    @cInclude("sys/types.h");
    @cInclude("sys/stat.h");
    @cInclude("syslog.h");
});

pub const builtin = @import("builtin");
pub const std = @import("std");
pub const warn = std.debug.warn;
pub const assert = std.debug.assert;
pub const expect = std.testing.expect;
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
    var writer = buf.writer();
    const message: []const u8 = message: {
        std.fmt.format(writer, fmt, args) catch |err| {
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
    return std.HashMap(K, V, meta.DeepHashContext(K), std.hash_map.DefaultMaxLoadPercentage);
}

pub fn dump(thing: anytype) void {
    const held = std.debug.getStderrMutex().acquire();
    defer held.release();
    const my_stderr = std.io.getStdErr();
    const writer = my_stderr.writer();
    dumpInto(writer, 0, thing) catch return;
    writer.writeAll("\n") catch return;
}

pub fn dumpInto(writer: anytype, indent: u32, thing: anytype) anyerror!void {
    switch (@typeInfo(@TypeOf(thing))) {
        .Pointer => |pti| {
            switch (pti.size) {
                .One => {
                    try writer.writeAll("&");
                    try dumpInto(writer, indent, thing.*);
                },
                .Many => {
                    // bail
                    try std.fmt.format(writer, "{}", .{thing});
                },
                .Slice => {
                    if (pti.child == u8) {
                        try std.fmt.format(writer, "\"{s}\"", .{thing});
                    } else {
                        try std.fmt.format(writer, "[]{s}[\n", .{pti.child});
                        for (thing) |elem| {
                            try writer.writeByteNTimes(' ', indent + 4);
                            try dumpInto(writer, indent + 4, elem);
                            try writer.writeAll(",\n");
                        }
                        try writer.writeByteNTimes(' ', indent);
                        try writer.writeAll("]");
                    }
                },
                .C => {
                    // bail
                    try std.fmt.format(writer, "{}", .{thing});
                },
            }
        },
        .Array => |ati| {
            if (ati.child == u8) {
                try std.fmt.format(writer, "\"{s}\"", .{thing});
            } else {
                try std.fmt.format(writer, "[{}]{s}[\n", .{ ati.len, ati.child });
                for (thing) |elem| {
                    try writer.writeByteNTimes(' ', indent + 4);
                    try dumpInto(writer, indent + 4, elem);
                    try writer.writeAll(",\n");
                }
                try writer.writeByteNTimes(' ', indent);
                try writer.writeAll("]");
            }
        },
        .Struct => |sti| {
            try writer.writeAll(@typeName(@TypeOf(thing)));
            try writer.writeAll("{\n");
            inline for (sti.fields) |field| {
                try writer.writeByteNTimes(' ', indent + 4);
                try std.fmt.format(writer, ".{s} = ", .{field.name});
                try dumpInto(writer, indent + 4, @field(thing, field.name));
                try writer.writeAll(",\n");
            }
            try writer.writeByteNTimes(' ', indent);
            try writer.writeAll("}");
        },
        .Union => |uti| {
            if (uti.tag_type) |tag_type| {
                try writer.writeAll(@typeName(@TypeOf(thing)));
                try writer.writeAll("{\n");
                inline for (@typeInfo(tag_type).Enum.fields) |fti| {
                    if (@enumToInt(std.meta.activeTag(thing)) == fti.value) {
                        try writer.writeByteNTimes(' ', indent + 4);
                        try std.fmt.format(writer, ".{s} = ", .{fti.name});
                        try dumpInto(writer, indent + 4, @field(thing, fti.name));
                        try writer.writeAll("\n");
                        try writer.writeByteNTimes(' ', indent);
                        try writer.writeAll("}");
                    }
                }
            } else {
                // bail
                try std.fmt.format(writer, "{}", .{thing});
            }
        },
        .Optional => {
            if (thing == null) {
                try writer.writeAll("null");
            } else {
                try dumpInto(writer, indent, thing.?);
            }
        },
        else => {
            // bail
            try std.fmt.format(writer, "{}", .{thing});
        },
    }
}

pub fn format(allocator: *Allocator, comptime fmt: []const u8, args: anytype) []const u8 {
    var buf = ArrayList(u8).init(allocator);
    var writer = buf.writer();
    std.fmt.format(writer, fmt, args) catch oom();
    return buf.items;
}

pub fn tagEqual(a: anytype, b: @TypeOf(a)) bool {
    return std.meta.activeTag(a) == std.meta.activeTag(b);
}

pub fn FixedSizeArrayList(comptime size: usize, comptime T: type) type {
    return struct {
        elems: [size]T,
        len: usize,

        const Self = @This();

        pub fn init() Self {
            return Self{
                .elems = undefined,
                .len = 0,
            };
        }

        pub fn append(self: *Self, elem: T) void {
            assert(self.len < size);
            self.elems[self.len] = elem;
            self.len += 1;
        }

        pub fn slice(self: *Self) []T {
            return self.elems[0..self.len];
        }
    };
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

    pub fn hsl(h: f64, s: f64, l: f64) Color {
        assert(h >= 0 and h < 360);
        const ch = (1 - @fabs((2 * l) - 1)) * s;
        const x = ch * (1 - @fabs(@mod(h / 60, 2) - 1));
        const m = l - (ch / 2);
        const rgb: [3]f64 = switch (@floatToInt(u8, @floor(h / 60))) {
            0 => .{ ch, x, 0 },
            1 => .{ x, ch, 0 },
            2 => .{ 0, ch, x },
            3 => .{ 0, x, ch },
            4 => .{ x, 0, ch },
            5 => .{ ch, 0, x },
            else => unreachable,
        };
        return .{
            .r = @floatToInt(u8, @round(255 * (rgb[0] + m))),
            .g = @floatToInt(u8, @round(255 * (rgb[1] + m))),
            .b = @floatToInt(u8, @round(255 * (rgb[2] + m))),
            .a = 255,
        };
    }
};

test "hsl" {
    // source https://www.rapidtables.com/convert/color/hsl-to-rgb.html
    try expect(meta.deepEqual(Color.hsl(0, 0, 0), Color{ .r = 0, .g = 0, .b = 0, .a = 255 }));
    try expect(meta.deepEqual(Color.hsl(0, 0, 1), Color{ .r = 255, .g = 255, .b = 255, .a = 255 }));
    try expect(meta.deepEqual(Color.hsl(0, 1, 0.5), Color{ .r = 255, .g = 0, .b = 0, .a = 255 }));
    try expect(meta.deepEqual(Color.hsl(120, 1, 0.5), Color{ .r = 0, .g = 255, .b = 0, .a = 255 }));
    try expect(meta.deepEqual(Color.hsl(240, 1, 0.5), Color{ .r = 0, .g = 0, .b = 255, .a = 255 }));
    try expect(meta.deepEqual(Color.hsl(60, 1, 0.5), Color{ .r = 255, .g = 255, .b = 0, .a = 255 }));
    try expect(meta.deepEqual(Color.hsl(180, 1, 0.5), Color{ .r = 0, .g = 255, .b = 255, .a = 255 }));
    try expect(meta.deepEqual(Color.hsl(300, 1, 0.5), Color{ .r = 255, .g = 0, .b = 255, .a = 255 }));
    try expect(meta.deepEqual(Color.hsl(0, 0, 0.75), Color{ .r = 191, .g = 191, .b = 191, .a = 255 }));
    try expect(meta.deepEqual(Color.hsl(0, 0, 0.5), Color{ .r = 128, .g = 128, .b = 128, .a = 255 }));
    try expect(meta.deepEqual(Color.hsl(0, 1, 0.25), Color{ .r = 128, .g = 0, .b = 0, .a = 255 }));
    try expect(meta.deepEqual(Color.hsl(120, 1, 0.25), Color{ .r = 0, .g = 128, .b = 0, .a = 255 }));
    try expect(meta.deepEqual(Color.hsl(240, 1, 0.25), Color{ .r = 0, .g = 0, .b = 128, .a = 255 }));
    try expect(meta.deepEqual(Color.hsl(60, 1, 0.25), Color{ .r = 128, .g = 128, .b = 0, .a = 255 }));
    try expect(meta.deepEqual(Color.hsl(180, 1, 0.25), Color{ .r = 0, .g = 128, .b = 128, .a = 255 }));
    try expect(meta.deepEqual(Color.hsl(300, 1, 0.25), Color{ .r = 128, .g = 0, .b = 128, .a = 255 }));
}

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
