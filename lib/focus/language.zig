const std = @import("std");
const focus = @import("../focus.zig");
const u = focus.util;
const c = focus.util.c;
const style = focus.style;

pub const zig = @import("./language/zig.zig");
pub const clojure = @import("./language/clojure.zig");

pub const Language = union(enum) {
    Zig: zig.State,
    Clojure: clojure.State,
    Java,
    Shell,
    Julia,
    Javascript,
    Nix,
    Unknown,

    pub fn init(allocator: u.Allocator, filename: []const u8, source: []const u8) Language {
        // TODO writing this as `return if ...` causes a confusing compiler error
        if (std.mem.endsWith(u8, filename, ".zig"))
            return .{ .Zig = zig.State.init(allocator, source) }
        else if (std.mem.endsWith(u8, filename, ".clj") or std.mem.endsWith(u8, filename, ".cljs") or std.mem.endsWith(u8, filename, ".cljc"))
            return .{ .Clojure = clojure.State.init(allocator, source) }
        else if (std.mem.endsWith(u8, filename, ".java"))
            return .Java
        else if (std.mem.endsWith(u8, filename, ".sh"))
            return .Shell
        else if (std.mem.endsWith(u8, filename, ".jl"))
            return .Julia
        else if (std.mem.endsWith(u8, filename, ".js"))
            return .Javascript
        else if (std.mem.endsWith(u8, filename, ".nix"))
            return .Nix
        else
            return .Unknown;
    }

    pub fn deinit(self: *Language) void {
        switch (self.*) {
            .Zig => |*state| state.deinit(),
            .Clojure => |*state| state.deinit(),
            else => {},
        }
    }

    pub fn updateBeforeChange(self: *Language, source: []const u8, delete_range: [2]usize) void {
        switch (self.*) {
            .Zig => |*state| state.updateBeforeChange(source, delete_range),
            .Clojure => |*state| state.updateBeforeChange(source, delete_range),
            else => {},
        }
    }

    pub fn updateAfterChange(self: *Language, source: []const u8, insert_range: [2]usize) void {
        switch (self.*) {
            .Zig => |*state| state.updateAfterChange(source, insert_range),
            .Clojure => |*state| state.updateAfterChange(source, insert_range),
            else => {},
        }
    }

    pub fn commentString(self: Language) ?[]const u8 {
        return switch (self) {
            .Zig, .Java, .Javascript => "//",
            .Shell, .Julia, .Nix => "#",
            .Clojure => ";",
            .Unknown => null,
        };
    }

    pub fn extendRangeToLineBoundary(source: []const u8, range: [2]usize) [2]usize {
        var extended_range = range;
        while (extended_range[0] > 0 and source[extended_range[0] - 1] != '\n') extended_range[0] -= 1;
        while (extended_range[1] < source.len and source[extended_range[1]] != '\n') extended_range[1] += 1;
        return extended_range;
    }

    pub fn highlight(self: Language, allocator: u.Allocator, source: []const u8, init_range: [2]usize) []const u.Color {
        const range = extendRangeToLineBoundary(source, init_range);
        const colors = allocator.alloc(u.Color, range[1] - range[0]) catch u.oom();
        std.mem.set(u.Color, colors, style.text_color);
        switch (self) {
            .Zig => |state| state.highlight(source, range, colors),
            .Clojure => |state| state.highlight(source, range, colors),
            else => {},
        }
        return colors[init_range[0] - range[0] ..];
    }

    fn isLikeIdent(self: Language, char: u8, is_first_char: bool) bool {
        _ = self;
        return switch (char) {
            'a'...'z', 'A'...'Z', '_' => true,
            '0'...'9' => !is_first_char,
            else => false,
        };
    }

    pub fn getTokenRanges(self: Language, allocator: u.Allocator, source: []const u8, init_range: [2]usize) []const [2]usize {
        const range = extendRangeToLineBoundary(source, init_range);
        var token_ranges = u.ArrayList([2]usize).init(allocator);
        defer token_ranges.deinit();
        switch (self) {
            .Zig => |state| {
                for (state.token_ranges) |token_range| {
                    if (token_range[1] < range[0] or token_range[0] > range[1]) continue;
                    token_ranges.append(token_range) catch u.oom();
                }
            },
            .Clojure => |state| {
                for (state.token_ranges) |token_range| {
                    if (token_range[1] < range[0] or token_range[0] > range[1]) continue;
                    token_ranges.append(token_range) catch u.oom();
                }
            },
            else => {
                var start: usize = range[0];
                while (start < range[1]) {
                    var end = start;
                    while (end < range[1] and self.isLikeIdent(source[end], start == end)) : (end += 1) {}
                    if (end > start) token_ranges.append(.{ start, end }) catch u.oom();
                    start = end + 1;
                    while (start < range[1] and !self.isLikeIdent(source[start], start == end)) : (start += 1) {}
                }
            },
        }
        return token_ranges.toOwnedSlice();
    }

    pub fn getTokens(self: Language, allocator: u.Allocator, source: []const u8, range: [2]usize) []const []const u8 {
        const token_ranges = self.getTokenRanges(allocator, source, range);
        defer allocator.free(token_ranges);
        var tokens = u.ArrayList([]const u8).init(allocator);
        for (token_ranges) |token_range| {
            const token = allocator.dupe(u8, source[token_range[0]..token_range[1]]) catch u.oom();
            tokens.append(token) catch u.oom();
        }
        return tokens.toOwnedSlice();
    }

    pub fn getTokenRangeAround(self: Language, allocator: u.Allocator, source: []const u8, pos: usize) ?[2]usize {
        const token_ranges = self.getTokenRanges(allocator, source, .{ pos, pos });
        defer allocator.free(token_ranges);
        for (token_ranges) |token_range| {
            if (token_range[0] <= pos and pos <= token_range[1]) return token_range;
        }
        return null;
    }

    pub fn format(self: Language, source: []const u8) ?[]const u8 {
        return switch (self) {
            .Zig => |state| state.format(source),
            else => null,
        };
    }
};
