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

    pub fn toggleMode(self: *Language) void {
        switch (self.*) {
            .Zig => |*state| state.toggleMode(),
            .Clojure => |*state| state.toggleMode(),
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

    pub fn highlight(self: Language, allocator: u.Allocator, source: []const u8, range: [2]usize) []const u.Color {
        const colors = allocator.alloc(u.Color, range[1] - range[0]) catch u.oom();
        std.mem.set(u.Color, colors, style.text_color);
        switch (self) {
            .Zig => |state| state.highlight(source, range, colors),
            .Clojure => |state| state.highlight(source, range, colors),
            else => {},
        }
        return colors;
    }

    pub fn getTokenRanges(self: Language) []const [2]usize {
        return switch (self) {
            .Zig => |state| state.token_ranges,
            .Clojure => |state| state.token_ranges,
            else => &.{},
        };
    }

    pub fn getParenMatches(self: Language) []const ?usize {
        return switch (self) {
            .Zig => |state| state.paren_matches,
            .Clojure => |state| state.paren_matches,
            else => &.{},
        };
    }

    pub fn getTokenIxBefore(self: Language, pos: usize) ?usize {
        const token_ranges = self.getTokenRanges();
        for (token_ranges) |token_range, i| {
            if (token_range[0] < pos and pos <= token_range[1]) return i;
        }
        return null;
    }

    pub fn getTokenIxAfter(self: Language, pos: usize) ?usize {
        const token_ranges = self.getTokenRanges();
        for (token_ranges) |token_range, i| {
            if (token_range[0] <= pos and pos < token_range[1]) return i;
        }
        return null;
    }

    pub fn format(self: Language, source: []const u8) ?[]const u8 {
        return switch (self) {
            .Zig => |state| state.format(source),
            else => null,
        };
    }

    pub fn matchParen(self: Language, pos: usize) ?usize {
        if (self.getTokenIxAfter(pos)) |token_ix|
            if (self.getParenMatches()[token_ix]) |matching_ix|
                return self.getTokenRanges()[matching_ix][0];
        return null;
    }
};
