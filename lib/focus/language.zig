const std = @import("std");
const focus = @import("../focus.zig");
const u = focus.util;
const c = focus.util.c;
const style = focus.style;

pub const zig = @import("./language/zig.zig");
pub const clojure = @import("./language/clojure.zig");
pub const generic = @import("./language/generic.zig");

pub const Language = union(enum) {
    Zig: zig.State,
    Clojure: clojure.State,
    Java: generic.State,
    Shell: generic.State,
    Julia: generic.State,
    Javascript: generic.State,
    Nix: generic.State,
    Unknown,

    pub fn init(allocator: u.Allocator, filename: []const u8, source: []const u8) Language {
        // TODO writing this as `return if ...` causes a confusing compiler error
        if (std.mem.endsWith(u8, filename, ".zig"))
            return .{ .Zig = zig.State.init(allocator, source) }
        else if (std.mem.endsWith(u8, filename, ".clj") or
            std.mem.endsWith(u8, filename, ".cljs") or
            std.mem.endsWith(u8, filename, ".cljc") or
            std.mem.endsWith(u8, filename, ".edn"))
            return .{ .Clojure = clojure.State.init(allocator, source) }
        else if (std.mem.endsWith(u8, filename, ".java"))
            return .{ .Java = generic.State.init(allocator, "//", source) }
        else if (std.mem.endsWith(u8, filename, ".sh"))
            return .{ .Shell = generic.State.init(allocator, "#", source) }
        else if (std.mem.endsWith(u8, filename, ".jl"))
            return .{ .Julia = generic.State.init(allocator, "#", source) }
        else if (std.mem.endsWith(u8, filename, ".js"))
            return .{ .Javascript = generic.State.init(allocator, "//", source) }
        else if (std.mem.endsWith(u8, filename, ".nix"))
            return .{ .Nix = generic.State.init(allocator, "#", source) }
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
            .Java, .Shell, .Julia, .Javascript, .Nix => |*state| state.updateBeforeChange(source, delete_range),
            .Unknown => {},
        }
    }

    pub fn updateAfterChange(self: *Language, source: []const u8, insert_range: [2]usize) void {
        switch (self.*) {
            .Zig => |*state| state.updateAfterChange(source, insert_range),
            .Clojure => |*state| state.updateAfterChange(source, insert_range),
            .Java, .Shell, .Julia, .Javascript, .Nix => |*state| state.updateAfterChange(source, insert_range),
            .Unknown => {},
        }
    }

    pub fn toggleMode(self: *Language) void {
        switch (self.*) {
            .Zig => |*state| state.toggleMode(),
            .Clojure => |*state| state.toggleMode(),
            .Java, .Shell, .Julia, .Javascript, .Nix => |*state| state.toggleMode(),
            .Unknown => {},
        }
    }

    pub fn getCommentString(self: Language) ?[]const u8 {
        return switch (self) {
            .Zig => "//",
            .Clojure => ";",
            .Java, .Shell, .Julia, .Javascript, .Nix => |*state| state.comment_string,
            .Unknown => null,
        };
    }

    pub fn highlight(self: Language, allocator: u.Allocator, source: []const u8, range: [2]usize) []const u.Color {
        const colors = allocator.alloc(u.Color, range[1] - range[0]) catch u.oom();
        std.mem.set(u.Color, colors, style.text_color);
        switch (self) {
            .Zig => |state| state.highlight(source, range, colors),
            .Clojure => |state| state.highlight(source, range, colors),
            .Java, .Shell, .Julia, .Javascript, .Nix => |state| state.highlight(source, range, colors),
            .Unknown => {},
        }
        return colors;
    }

    pub fn getTokenRanges(self: Language) []const [2]usize {
        return switch (self) {
            .Zig => |state| state.token_ranges,
            .Clojure => |state| state.token_ranges,
            .Java, .Shell, .Julia, .Javascript, .Nix => |state| state.token_ranges,
            .Unknown => &[0][2]usize{},
        };
    }

    pub fn getParenLevels(self: Language) []const usize {
        return switch (self) {
            .Zig => |state| state.paren_levels,
            .Clojure => |state| state.paren_levels,
            .Java, .Shell, .Julia, .Javascript, .Nix => |state| state.paren_levels,
            .Unknown => &[0]?usize{},
        };
    }

    pub fn getParenParents(self: Language) []const ?usize {
        return switch (self) {
            .Zig => |state| state.paren_parents,
            .Clojure => |state| state.paren_parents,
            .Java, .Shell, .Julia, .Javascript, .Nix => |state| state.paren_parents,
            .Unknown => &[0]?usize{},
        };
    }

    pub fn getParenMatches(self: Language) []const ?usize {
        return switch (self) {
            .Zig => |state| state.paren_matches,
            .Clojure => |state| state.paren_matches,
            .Java, .Shell, .Julia, .Javascript, .Nix => |state| state.paren_matches,
            .Unknown => &[0]?usize{},
        };
    }

    pub fn getTokenIxBefore(self: Language, pos: usize) ?usize {
        const token_ranges = self.getTokenRanges();
        var return_ix: ?usize = null;
        for (token_ranges) |token_range, ix| {
            if (token_range[0] >= pos) break;
            return_ix = ix;
        }
        return return_ix;
    }

    pub fn getTokenIxAfter(self: Language, pos: usize) ?usize {
        const token_ranges = self.getTokenRanges();
        for (token_ranges) |token_range, ix| {
            if (token_range[0] >= pos)
                return ix;
        }
        return null;
    }

    pub fn format(self: Language, source: []const u8) ?[]const u8 {
        return switch (self) {
            .Zig => |state| state.format(source),
            .Clojure => |state| stripTrailingWhitespace(state.allocator, source),
            .Java, .Shell, .Julia, .Javascript, .Nix => |state| stripTrailingWhitespace(state.allocator, source),
            .Unknown => null,
        };
    }

    pub fn stripTrailingWhitespace(allocator: u.Allocator, source: []const u8) []const u8 {
        var new_source = u.ArrayList(u8).init(allocator);
        defer new_source.deinit();
        var line_start: usize = 0;
        var line_end: usize = 0;
        var i: usize = 0;
        while (true) {
            if (i >= source.len) {
                new_source.appendSlice(source[line_start..line_end]) catch u.oom();
                break;
            } else if (source[i] == '\n') {
                new_source.appendSlice(source[line_start..line_end]) catch u.oom();
                new_source.append('\n') catch u.oom();
                i += 1;
                line_start = i;
                line_end = i;
            } else {
                if (source[i] != ' ')
                    line_end = i + 1;
                i += 1;
            }
        }
        return new_source.toOwnedSlice();
    }

    pub fn matchParen(self: Language, pos: usize) ?usize {
        if (self.getTokenIxBefore(pos)) |token_ix| {
            const token_range = self.getTokenRanges()[token_ix];
            if (token_range[1] >= pos)
                if (self.getParenMatches()[token_ix]) |matching_ix|
                    return self.getTokenRanges()[matching_ix][1];
        }
        return null;
    }

    pub fn getAddedIndent(self: Language, token_ix: usize) usize {
        return switch (self) {
            .Zig => |state| state.getAddedIndent(token_ix),
            .Clojure => |state| state.getAddedIndent(token_ix),
            .Java, .Shell, .Julia, .Javascript, .Nix => |state| state.getAddedIndent(token_ix),
            .Unknown => 0,
        };
    }

    pub fn getIdealIndent(self: Language, source: []const u8, line_start_pos: usize) usize {
        const anchor: struct {
            ix: usize,
            added_indent: usize,
        } = anchor: {
            if (self.getTokenIxAfter(line_start_pos)) |after_ix|
                if (self.getParenMatches()[after_ix]) |matching_ix|
                    if (matching_ix < after_ix)
                        // line starts with closing paren
                        // align with the opening paren
                        break :anchor .{ .ix = matching_ix, .added_indent = 0 };

            if (self.getTokenIxBefore(line_start_pos)) |before_ix| {
                const added_indent = self.getAddedIndent(before_ix);
                if (added_indent != 0)
                    // prev line ends with opening paren
                    // indent from the opening paren
                    break :anchor .{ .ix = before_ix, .added_indent = added_indent };

                if (self.getParenParents()[before_ix]) |parent_ix|
                    // prev line is in some paren pair
                    // indent with the opening paren of that pair
                    break :anchor .{ .ix = parent_ix, .added_indent = self.getAddedIndent(parent_ix) };
            }

            // nothing to align with
            // give up
            return 0;
        };

        const anchor_range = self.getTokenRanges()[anchor.ix];
        const anchor_line_start = if (std.mem.lastIndexOfScalar(u8, source[0..anchor_range[0]], '\n')) |n|
            n + 1
        else
            0;
        var anchor_text_start = anchor_line_start;
        // TODO handle alternative whitespace
        while (anchor_text_start < anchor_range[0] and source[anchor_text_start] == ' ') : (anchor_text_start += 1) {}
        const anchor_indent = anchor_text_start - anchor_line_start;
        const anchor_pos = anchor_range[1] - 1 - anchor_line_start;

        return switch (self) {
            .Clojure => anchor_pos + anchor.added_indent,
            else => anchor_indent + anchor.added_indent,
        };
    }
};
