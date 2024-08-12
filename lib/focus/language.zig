const std = @import("std");
const focus = @import("../focus.zig");
const u = focus.util;
const c = focus.util.c;
const style = focus.style;

pub const zig = @import("./language/zig.zig");
pub const clojure = @import("./language/clojure.zig");
pub const deno = @import("./language/deno.zig");
pub const go = @import("./language/go.zig");
pub const generic = @import("./language/generic.zig");

pub const Language = union(enum) {
    Zig: zig.State,
    Clojure: clojure.State,
    Java: generic.State,
    Shell: generic.State,
    Julia: generic.State,
    Javascript: deno.State,
    Typescript: deno.State,
    Nix: generic.State,
    C: generic.State,
    Go: go.State,
    Zest: generic.State,
    Unknown,

    pub const Squiggly = struct {
        color: u.Color,
        range: [2]usize,
    };

    pub fn init(allocator: u.Allocator, filename: []const u8, source: []const u8) Language {
        return if (std.mem.endsWith(u8, filename, ".zig"))
            .{ .Zig = zig.State.init(allocator, source) }
        else if (std.mem.endsWith(u8, filename, ".clj") or
            std.mem.endsWith(u8, filename, ".cljs") or
            std.mem.endsWith(u8, filename, ".cljc") or
            std.mem.endsWith(u8, filename, ".edn"))
            .{ .Clojure = clojure.State.init(allocator, source) }
        else if (std.mem.endsWith(u8, filename, ".java"))
            .{ .Java = generic.State.init(allocator, "//", source) }
        else if (std.mem.endsWith(u8, filename, ".sh"))
            .{ .Shell = generic.State.init(allocator, "#", source) }
        else if (std.mem.endsWith(u8, filename, ".jl"))
            .{ .Julia = generic.State.init(allocator, "#", source) }
        else if (std.mem.endsWith(u8, filename, ".js"))
            .{ .Javascript = deno.State.init(allocator, source) }
        else if (std.mem.endsWith(u8, filename, ".ts"))
            .{ .Typescript = deno.State.init(allocator, source) }
        else if (std.mem.endsWith(u8, filename, ".nix"))
            .{ .Nix = generic.State.init(allocator, "#", source) }
        else if (std.mem.endsWith(u8, filename, ".c") or std.mem.endsWith(u8, filename, ".h"))
            .{ .C = generic.State.init(allocator, "//", source) }
        else if (std.mem.endsWith(u8, filename, ".go") or (std.mem.endsWith(u8, filename, ".go.tmpl")))
            .{ .Go = go.State.init(allocator, source) }
        else if (std.mem.endsWith(u8, filename, ".zs"))
            .{ .Zest = generic.State.init(allocator, "//", source) }
        else
            .Unknown;
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
            .Unknown => {},
            inline else => |*state| state.updateBeforeChange(source, delete_range),
        }
    }

    pub fn updateAfterChange(self: *Language, source: []const u8, insert_range: [2]usize) void {
        switch (self.*) {
            .Unknown => {},
            inline else => |*state| state.updateAfterChange(source, insert_range),
        }
    }

    pub fn toggleMode(self: *Language) void {
        switch (self.*) {
            .Unknown => {},
            inline else => |*state| state.toggleMode(),
        }
    }

    pub fn getCommentString(self: Language) ?[]const u8 {
        return switch (self) {
            .Zig => "//",
            .Clojure => ";",
            .Javascript, .Typescript, .Go => "//",
            .Java, .Shell, .Julia, .Nix, .C, .Zest => |*state| state.comment_string,
            .Unknown => null,
        };
    }

    pub fn highlight(self: Language, allocator: u.Allocator, source: []const u8, range: [2]usize) []const u.Color {
        const colors = allocator.alloc(u.Color, range[1] - range[0]) catch u.oom();
        @memset(colors, style.text_color);
        switch (self) {
            .Unknown => {},
            inline else => |state| state.highlight(source, range, colors),
        }
        return colors;
    }

    pub fn getTokenRanges(self: Language) []const [2]usize {
        return switch (self) {
            inline .Javascript, .Typescript, .Go => |state| state.generic.token_ranges,
            .Unknown => &[0][2]usize{},
            inline else => |state| state.token_ranges,
        };
    }

    pub fn getParenLevels(self: Language) []const usize {
        return switch (self) {
            inline .Javascript, .Typescript, .Go => |state| state.generic.paren_levels,
            .Unknown => &[0]?usize{},
            inline else => |state| state.paren_levels,
        };
    }

    pub fn getParenParents(self: Language) []const ?usize {
        return switch (self) {
            inline .Javascript, .Typescript, .Go => |state| state.generic.paren_parents,
            .Unknown => &[0]?usize{},
            inline else => |state| state.paren_parents,
        };
    }

    pub fn getParenMatches(self: Language) []const ?usize {
        return switch (self) {
            inline .Javascript, .Typescript, .Go => |state| state.generic.paren_matches,
            .Unknown => &[0]?usize{},
            inline else => |state| state.paren_matches,
        };
    }

    pub fn getTokenIxBefore(self: Language, pos: usize) ?usize {
        const token_ranges = self.getTokenRanges();
        var return_ix: ?usize = null;
        for (token_ranges, 0..) |token_range, ix| {
            if (token_range[0] >= pos) break;
            return_ix = ix;
        }
        return return_ix;
    }

    pub fn getTokenIxAfter(self: Language, pos: usize) ?usize {
        const token_ranges = self.getTokenRanges();
        for (token_ranges, 0..) |token_range, ix| {
            if (token_range[0] >= pos)
                return ix;
        }
        return null;
    }

    pub fn getIdentifierRangeAt(self: Language, pos: usize) ?[2]usize {
        const token_ix = self.getTokenIxBefore(pos) orelse self.getTokenIxAfter(pos) orelse return null;
        switch (self) {
            inline .Zig, .Zest => |state| return if (state.tokens[token_ix] == .identifier) (state.token_ranges[token_ix]) else null,
            .Go => |state| return if (state.generic.tokens[token_ix] == .identifier) (state.generic.token_ranges[token_ix]) else null,
            else => return null,
        }
    }

    pub fn format(self: Language, frame_allocator: u.Allocator, source: []const u8) ?[]const u8 {
        return switch (self) {
            inline .Zig, .Javascript, .Typescript, .Go => |state| state.format(frame_allocator, source),
            .Unknown => null,
            inline else => stripTrailingWhitespace(frame_allocator, source),
        };
    }

    pub fn stripTrailingWhitespace(frame_allocator: u.Allocator, source: []const u8) []const u8 {
        var new_source = u.ArrayList(u8).init(frame_allocator);

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
        return new_source.toOwnedSlice() catch u.oom();
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
            .Unknown => 0,
            inline else => |state| state.getAddedIndent(token_ix),
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

    pub fn getSquigglies(self: Language) []const Language.Squiggly {
        switch (self) {
            .Zig => |state| return state.squigglies,
            else => return &[0]Language.Squiggly{},
        }
    }

    pub fn afterLoad(self: Language, frame_allocator: u.Allocator, source: []const u8) []const u8 {
        switch (self) {
            .Go => |state| return state.afterLoad(frame_allocator, source),
            else => return source,
        }
    }

    pub fn beforeSave(self: Language, frame_allocator: u.Allocator, source: []const u8) []const u8 {
        return switch (self) {
            .Go => |state| return state.beforeSave(frame_allocator, source),
            else => source,
        };
    }

    pub fn writeDefinitionSearchString(self: Language, writer: anytype, identifier: []const u8) !void {
        switch (self) {
            .Zig => try std.fmt.format(
                writer,
                \\fn {s}\(|const {s} =|var {s} =| {s}:
            ,
                .{ identifier, identifier, identifier, identifier },
            ),
            .Go => try std.fmt.format(
                writer,
                \\func.* {s}\(|type {s}|\t{s} struct|\t{s} interface|\t{s} enum|\t{s} func|\t{s}\s*=
            ,
                .{ identifier, identifier, identifier, identifier, identifier, identifier, identifier },
            ),
            else => {},
        }
    }
};
