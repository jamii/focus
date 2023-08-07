const builtin = @import("builtin");
const std = @import("std");
const focus = @import("../focus.zig");

pub const c = @cImport({
    @cInclude("GL/gl.h");
    @cInclude("unistd.h");
});

pub const warn = std.log.warn;
pub const assert = std.debug.assert;
pub const expect = std.testing.expect;
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
        break :message buf.toOwnedSlice() catch |err| {
            switch (err) {
                error.OutOfMemory => break :message "OOM inside panic",
            }
        };
    };
    @panic(message);
}

pub fn oom() noreturn {
    @panic("Out of memory");
}

pub fn DeepHashMap(comptime K: type, comptime V: type) type {
    return std.HashMap(K, V, DeepHashContext(K), std.hash_map.default_max_load_percentage);
}

pub fn DeepHashSet(comptime K: type) type {
    return DeepHashMap(K, void);
}

pub fn dump(thing: anytype) void {
    std.debug.getStderrMutex().lock();
    defer std.debug.getStderrMutex().unlock();
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
                    if (@intFromEnum(std.meta.activeTag(thing)) == fti.value) {
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
        .Opaque => {
            try writer.writeAll("opaque");
        },
        else => {
            // bail
            try std.fmt.format(writer, "{any}", .{thing});
        },
    }
}

pub fn format(allocator: Allocator, comptime fmt: []const u8, args: anytype) []const u8 {
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

    pub fn hsla(h: f64, s: f64, l: f64, a: f64) Color {
        assert(h >= 0 and h < 360);
        assert(s >= 0 and s <= 1);
        assert(l >= 0 and l <= 1);
        assert(a >= 0 and a <= 1);
        const ch = (1 - @fabs((2 * l) - 1)) * s;
        const x = ch * (1 - @fabs(@mod(h / 60, 2) - 1));
        const m = l - (ch / 2);
        const rgb: [3]f64 = switch (@as(u8, @intFromFloat(@floor(h / 60)))) {
            0 => .{ ch, x, 0 },
            1 => .{ x, ch, 0 },
            2 => .{ 0, ch, x },
            3 => .{ 0, x, ch },
            4 => .{ x, 0, ch },
            5 => .{ ch, 0, x },
            else => unreachable,
        };
        return .{
            .r = @intFromFloat(@round(255 * (rgb[0] + m))),
            .g = @intFromFloat(@round(255 * (rgb[1] + m))),
            .b = @intFromFloat(@round(255 * (rgb[2] + m))),
            .a = @intFromFloat(@round(255 * a)),
        };
    }
};

test "hsl" {
    // source https://www.rapidtables.com/convert/color/hsl-to-rgb.html
    try expect(deepEqual(Color.hsl(0, 0, 0), Color{ .r = 0, .g = 0, .b = 0, .a = 255 }));
    try expect(deepEqual(Color.hsl(0, 0, 1), Color{ .r = 255, .g = 255, .b = 255, .a = 255 }));
    try expect(deepEqual(Color.hsl(0, 1, 0.5), Color{ .r = 255, .g = 0, .b = 0, .a = 255 }));
    try expect(deepEqual(Color.hsl(120, 1, 0.5), Color{ .r = 0, .g = 255, .b = 0, .a = 255 }));
    try expect(deepEqual(Color.hsl(240, 1, 0.5), Color{ .r = 0, .g = 0, .b = 255, .a = 255 }));
    try expect(deepEqual(Color.hsl(60, 1, 0.5), Color{ .r = 255, .g = 255, .b = 0, .a = 255 }));
    try expect(deepEqual(Color.hsl(180, 1, 0.5), Color{ .r = 0, .g = 255, .b = 255, .a = 255 }));
    try expect(deepEqual(Color.hsl(300, 1, 0.5), Color{ .r = 255, .g = 0, .b = 255, .a = 255 }));
    try expect(deepEqual(Color.hsl(0, 0, 0.75), Color{ .r = 191, .g = 191, .b = 191, .a = 255 }));
    try expect(deepEqual(Color.hsl(0, 0, 0.5), Color{ .r = 128, .g = 128, .b = 128, .a = 255 }));
    try expect(deepEqual(Color.hsl(0, 1, 0.25), Color{ .r = 128, .g = 0, .b = 0, .a = 255 }));
    try expect(deepEqual(Color.hsl(120, 1, 0.25), Color{ .r = 0, .g = 128, .b = 0, .a = 255 }));
    try expect(deepEqual(Color.hsl(240, 1, 0.25), Color{ .r = 0, .g = 0, .b = 128, .a = 255 }));
    try expect(deepEqual(Color.hsl(60, 1, 0.25), Color{ .r = 128, .g = 128, .b = 0, .a = 255 }));
    try expect(deepEqual(Color.hsl(180, 1, 0.25), Color{ .r = 0, .g = 128, .b = 128, .a = 255 }));
    try expect(deepEqual(Color.hsl(300, 1, 0.25), Color{ .r = 128, .g = 0, .b = 128, .a = 255 }));
}

pub const Vec2f = packed struct {
    x: f32,
    y: f32,
};

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
pub fn fuzzy_search(allocator: Allocator, items: []const []const u8, filter: []const u8) [][]const u8 {
    var scored_items = ArrayList(ScoredItem).init(allocator);
    defer scored_items.deinit();

    for (items) |item| {
        if (filter.len > 0) {
            var score: usize = std.math.maxInt(usize);
            var any_match = false;
            const filter_start_char = filter[0];
            for (item, 0..) |start_char, start| {
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
                        score = @min(score, end - start);
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
    std.mem.sort(ScoredItem, scored_items.items, {}, struct {
        fn lessThan(_: void, a: ScoredItem, b: ScoredItem) bool {
            return a.score < b.score;
        }
    }.lessThan);

    var results = ArrayList([]const u8).init(allocator);
    for (scored_items.items) |scored_item| {
        results.append(scored_item.item) catch oom();
    }
    return results.toOwnedSlice() catch oom();
}

pub fn fuzzy_search_paths(allocator: Allocator, path: []const u8) ![]const []const u8 {
    var results = ArrayList([]const u8).init(allocator);
    {
        var dirname_o: ?[]const u8 = null;
        var basename: []const u8 = "";
        if (path.len > 0) {
            if (std.fs.path.isSep(path[path.len - 1])) {
                dirname_o = path;
                basename = "";
            } else {
                dirname_o = std.fs.path.dirname(path);
                basename = std.fs.path.basename(path);
            }
        }
        if (dirname_o) |dirname| {
            var dir = try std.fs.cwd().openIterableDir(dirname, .{});
            defer dir.close();
            var dir_iter = dir.iterate();
            while (dir_iter.next() catch |err| panic("{} while iterating dir {s}", .{ err, dirname })) |entry| {
                if (std.mem.startsWith(u8, entry.name, basename)) {
                    var result = ArrayList(u8).init(allocator);
                    result.appendSlice(entry.name) catch oom();
                    if (entry.kind == .directory) result.append('/') catch oom();
                    results.append(result.toOwnedSlice() catch oom()) catch oom();
                }
            }
        }
    }
    std.mem.sort([]const u8, results.items, {}, struct {
        fn lessThan(_: void, a: []const u8, b: []const u8) bool {
            return std.mem.lessThan(u8, a, b);
        }
    }.lessThan);
    return results.toOwnedSlice() catch oom();
}

const Match = struct {
    matched: [2]usize,
    captures: []const [2]usize,
};

pub const Ordering = enum {
    LessThan,
    Equal,
    GreaterThan,
};

pub fn deepEqual(a: anytype, b: @TypeOf(a)) bool {
    return deepCompare(a, b) == .Equal;
}

pub fn deepCompare(a: anytype, b: @TypeOf(a)) Ordering {
    const T = @TypeOf(a);
    const ti = @typeInfo(T);
    switch (ti) {
        .Struct, .Enum, .Union => {
            if (@hasDecl(T, "deepCompare")) {
                return T.deepCompare(a, b);
            }
        },
        else => {},
    }
    switch (ti) {
        .Bool => {
            if (a == b) return .Equal;
            if (a) return .GreaterThan;
            return .LessThan;
        },
        .Int, .Float => {
            if (a < b) {
                return .LessThan;
            }
            if (a > b) {
                return .GreaterThan;
            }
            return .Equal;
        },
        .Enum => {
            return deepCompare(@intFromEnum(a), @intFromEnum(b));
        },
        .Pointer => |pti| {
            switch (pti.size) {
                .One => {
                    return deepCompare(a.*, b.*);
                },
                .Slice => {
                    if (a.len < b.len) {
                        return .LessThan;
                    }
                    if (a.len > b.len) {
                        return .GreaterThan;
                    }
                    for (a, 0..) |a_elem, a_ix| {
                        const ordering = deepCompare(a_elem, b[a_ix]);
                        if (ordering != .Equal) {
                            return ordering;
                        }
                    }
                    return .Equal;
                },
                .Many, .C => @compileError("cannot deepCompare " ++ @typeName(T)),
            }
        },
        .Optional => {
            if (a) |a_val| {
                if (b) |b_val| {
                    return deepCompare(a_val, b_val);
                } else {
                    return .GreaterThan;
                }
            } else {
                if (b) |_| {
                    return .LessThan;
                } else {
                    return .Equal;
                }
            }
        },
        .Array => {
            for (a, 0..) |a_elem, a_ix| {
                const ordering = deepCompare(a_elem, b[a_ix]);
                if (ordering != .Equal) {
                    return ordering;
                }
            }
            return .Equal;
        },
        .Struct => |sti| {
            inline for (sti.fields) |fti| {
                const ordering = deepCompare(@field(a, fti.name), @field(b, fti.name));
                if (ordering != .Equal) {
                    return ordering;
                }
            }
            return .Equal;
        },
        .Union => |uti| {
            if (uti.tag_type) |tag_type| {
                const enum_info = @typeInfo(tag_type).Enum;
                const a_tag = @intFromEnum(@as(tag_type, a));
                const b_tag = @intFromEnum(@as(tag_type, b));
                if (a_tag < b_tag) {
                    return .LessThan;
                }
                if (a_tag > b_tag) {
                    return .GreaterThan;
                }
                inline for (enum_info.fields) |fti| {
                    if (a_tag == fti.value) {
                        return deepCompare(
                            @field(a, fti.name),
                            @field(b, fti.name),
                        );
                    }
                }
                unreachable;
            } else {
                @compileError("cannot deepCompare " ++ @typeName(T));
            }
        },
        .Void => return .Equal,
        .ErrorUnion => {
            if (a) |a_ok| {
                if (b) |b_ok| {
                    return deepCompare(a_ok, b_ok);
                } else |_| {
                    return .LessThan;
                }
            } else |a_err| {
                if (b) |_| {
                    return .GreaterThan;
                } else |b_err| {
                    return deepCompare(a_err, b_err);
                }
            }
        },
        .ErrorSet => return deepCompare(@intFromError(a), @intFromError(b)),
        else => @compileError("cannot deepCompare " ++ @typeName(T)),
    }
}

pub fn deepHash(key: anytype) u64 {
    var hasher = std.hash.Wyhash.init(0);
    deepHashInto(&hasher, key);
    return hasher.final();
}

pub fn deepHashInto(hasher: anytype, key: anytype) void {
    const T = @TypeOf(key);
    const ti = @typeInfo(T);
    switch (ti) {
        .Struct, .Enum, .Union => {
            if (@hasDecl(T, "deepHashInto")) {
                return T.deepHashInto(hasher, key);
            }
        },
        else => {},
    }
    switch (ti) {
        .Int => @call(.always_inline, std.hash.Wyhash.update, .{ hasher, std.mem.asBytes(&key) }),
        .Float => |info| deepHashInto(hasher, @as(std.Int(.unsigned, info.bits), @bitCast(key))),
        .Bool => deepHashInto(hasher, @intFromBool(key)),
        .Enum => deepHashInto(hasher, @intFromEnum(key)),
        .Pointer => |pti| {
            switch (pti.size) {
                .One => deepHashInto(hasher, key.*),
                .Slice => {
                    for (key) |element| {
                        deepHashInto(hasher, element);
                    }
                },
                .Many, .C => @compileError("cannot deepHash " ++ @typeName(T)),
            }
        },
        .Optional => if (key) |k| deepHashInto(hasher, k),
        .Array => {
            for (key) |element| {
                deepHashInto(hasher, element);
            }
        },
        .Struct => |info| {
            inline for (info.fields) |field| {
                deepHashInto(hasher, @field(key, field.name));
            }
        },
        .Union => |info| {
            if (info.tag_type) |tag_type| {
                const enum_info = @typeInfo(tag_type).Enum;
                const tag = std.meta.activeTag(key);
                deepHashInto(hasher, tag);
                inline for (enum_info.fields) |enum_field| {
                    if (enum_field.value == @intFromEnum(tag)) {
                        deepHashInto(hasher, @field(key, enum_field.name));
                        return;
                    }
                }
                unreachable;
            } else @compileError("cannot deepHash " ++ @typeName(T));
        },
        .Void => {},
        else => @compileError("cannot deepHash " ++ @typeName(T)),
    }
}

pub fn DeepHashContext(comptime K: type) type {
    return struct {
        const Self = @This();
        pub fn hash(_: Self, pseudo_key: K) u64 {
            return deepHash(pseudo_key);
        }
        pub fn eql(_: Self, pseudo_key: K, key: K) bool {
            return deepEqual(pseudo_key, key);
        }
    };
}

pub const BinarySearchResult = union(enum) {
    Found: usize,
    NotFound: usize,

    pub fn position(self: BinarySearchResult) usize {
        return switch (self) {
            .Found => |found| found,
            .NotFound => |not_found| not_found,
        };
    }
};

pub fn binarySearch(
    comptime T: type,
    key: anytype,
    items: []const T,
    context: anytype,
    comptime compareFn: fn (context: @TypeOf(context), lhs: @TypeOf(key), rhs: T) std.math.Order,
) BinarySearchResult {
    var left: usize = 0;
    var right: usize = items.len;

    while (left < right) {
        // Avoid overflowing in the midpoint calculation
        const mid = left + (right - left) / 2;
        // Compare the key with the midpoint element
        switch (compareFn(context, key, items[mid])) {
            .eq => return .{ .Found = mid },
            .gt => left = mid + 1,
            .lt => right = mid,
        }
    }

    return .{ .NotFound = left };
}
