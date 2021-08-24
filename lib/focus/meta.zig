const focus = @import("../focus.zig");
usingnamespace focus.common;

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
            return deepCompare(@enumToInt(a), @enumToInt(b));
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
                    for (a) |a_elem, a_ix| {
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
            for (a) |a_elem, a_ix| {
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
                const a_tag = @enumToInt(@as(tag_type, a));
                const b_tag = @enumToInt(@as(tag_type, b));
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
        .ErrorSet => return deepCompare(@errorToInt(a), @errorToInt(b)),
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
        .Int => @call(.{ .modifier = .always_inline }, hasher.update, .{std.mem.asBytes(&key)}),
        .Float => |info| deepHashInto(hasher, @bitCast(std.meta.Int(.unsigned, info.bits), key)),
        .Bool => deepHashInto(hasher, @boolToInt(key)),
        .Enum => deepHashInto(hasher, @enumToInt(key)),
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
                    if (enum_field.value == @enumToInt(tag)) {
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
