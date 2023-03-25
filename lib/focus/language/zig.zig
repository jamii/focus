const std = @import("std");
const focus = @import("../../focus.zig");
const u = focus.util;
const c = focus.util.c;
const style = focus.style;
const Language = focus.Language;

pub const State = struct {
    allocator: u.Allocator,
    tokens: []const std.zig.Token.Tag,
    token_ranges: []const [2]usize,
    paren_levels: []const usize,
    paren_parents: []const ?usize,
    paren_matches: []const ?usize,
    mode: enum {
        Normal,
        NoStructure,
        Parens,
    },
    squigglies: []const Language.Squiggly,

    pub fn init(allocator: u.Allocator, source: []const u8) State {
        var tokens = u.ArrayList(std.zig.Token.Tag).init(allocator);
        var token_ranges = u.ArrayList([2]usize).init(allocator);
        const source_z = allocator.dupeZ(u8, source) catch u.oom();
        defer allocator.free(source_z);
        var tokenizer = std.zig.Tokenizer.init(source_z);
        while (true) {
            const token = tokenizer.next();
            if (token.tag == .eof) break;
            tokens.append(token.tag) catch u.oom();
            token_ranges.append(.{ token.loc.start, token.loc.end }) catch u.oom();
        }

        const paren_levels = allocator.alloc(usize, tokens.items.len) catch u.oom();
        std.mem.set(usize, paren_levels, 0);
        const paren_parents = allocator.alloc(?usize, tokens.items.len) catch u.oom();
        std.mem.set(?usize, paren_parents, null);
        const paren_matches = allocator.alloc(?usize, tokens.items.len) catch u.oom();
        std.mem.set(?usize, paren_matches, null);
        var paren_match_stack = u.ArrayList(usize).init(allocator);
        for (tokens.items, 0..) |token, ix| {
            switch (token) {
                .r_paren, .r_brace, .r_bracket => {
                    if (paren_match_stack.popOrNull()) |matching_ix| {
                        paren_matches[ix] = matching_ix;
                        paren_matches[matching_ix] = ix;
                    }
                },
                else => {},
            }
            if (paren_match_stack.items.len > 0)
                paren_parents[ix] = paren_match_stack.items[paren_match_stack.items.len - 1];
            paren_levels[ix] = paren_match_stack.items.len;
            switch (token) {
                .l_paren, .l_brace, .l_bracket => {
                    paren_match_stack.append(ix) catch u.oom();
                },
                else => {},
            }
        }

        var squigglies = u.ArrayList(Language.Squiggly).init(allocator);
        {
            var line_start: usize = 0;
            while (line_start < source.len) {
                var line_end = line_start;
                while (line_end < source.len and source[line_end] != '\n') : (line_end += 1) {}
                if (line_end - line_start >= 100)
                    squigglies.append(.{
                        .color = style.emphasisOrange,
                        .range = .{ line_start + 99, line_end },
                    }) catch u.oom();
                line_start = line_end + 1;
            }
        }

        return .{
            .allocator = allocator,
            .tokens = tokens.toOwnedSlice() catch u.oom(),
            .token_ranges = token_ranges.toOwnedSlice() catch u.oom(),
            .paren_levels = paren_levels,
            .paren_parents = paren_parents,
            .paren_matches = paren_matches,
            .mode = .NoStructure,
            .squigglies = squigglies.toOwnedSlice() catch u.oom(),
        };
    }

    pub fn deinit(self: *State) void {
        self.allocator.free(self.paren_matches);
        self.allocator.free(self.paren_parents);
        self.allocator.free(self.paren_levels);
        self.allocator.free(self.token_ranges);
        self.allocator.free(self.tokens);
        self.* = undefined;
    }

    pub fn updateBeforeChange(self: *State, source: []const u8, delete_range: [2]usize) void {
        _ = self;
        _ = source;
        _ = delete_range;
    }

    pub fn updateAfterChange(self: *State, source: []const u8, insert_range: [2]usize) void {
        _ = insert_range;
        const allocator = self.allocator;
        const mode = self.mode;
        self.deinit();
        self.* = State.init(allocator, source);
        self.mode = mode;
    }

    pub fn toggleMode(self: *State) void {
        self.mode = switch (self.mode) {
            .Normal => .NoStructure,
            .NoStructure => .Parens,
            .Parens => .Normal,
        };
    }

    pub fn highlight(self: State, source: []const u8, range: [2]usize, colors: []u.Color) void {
        std.mem.set(u.Color, colors, style.comment_color);
        for (self.token_ranges, 0..) |token_range, i| {
            const source_start = token_range[0];
            const source_end = token_range[1];
            if (source_end < range[0] or source_start > range[1]) continue;
            const colors_start = if (source_start > range[0]) source_start - range[0] else 0;
            const colors_end = if (source_end > range[1]) range[1] - range[0] else source_end - range[0];
            const token = self.tokens[i];
            const structure_color = if (self.mode == .Normal)
                style.keyword_color
            else
                style.comment_color;
            const color = switch (token) {
                .doc_comment, .container_doc_comment => style.comment_color,
                .identifier, .builtin, .number_literal => if (self.mode == .Parens) style.comment_color else style.identColor(source[source_start..source_end]),
                .keyword_try, .keyword_catch, .keyword_error => if (self.mode == .Parens) style.comment_color else style.emphasisRed,
                .keyword_defer, .keyword_errdefer => if (self.mode == .Parens) style.comment_color else style.emphasisOrange,
                .keyword_break, .keyword_continue, .keyword_return => if (self.mode == .Parens) style.comment_color else style.emphasisGreen,
                .l_paren, .l_brace, .l_bracket, .r_paren, .r_brace, .r_bracket => color: {
                    var is_good_match = false;
                    if (self.paren_matches[i]) |matching_ix| {
                        const matching_token = self.tokens[matching_ix];
                        is_good_match = switch (token) {
                            .l_paren => matching_token == .r_paren,
                            .l_brace => matching_token == .r_brace,
                            .l_bracket => matching_token == .r_bracket,
                            .r_paren => matching_token == .l_paren,
                            .r_brace => matching_token == .l_brace,
                            .r_bracket => matching_token == .l_bracket,
                            else => unreachable,
                        };
                    }
                    break :color if (is_good_match)
                        if (self.mode == .Parens)
                            style.parenColor(self.paren_levels[i])
                        else
                            structure_color
                    else
                        style.emphasisRed;
                },
                .pipe, .equal_angle_bracket_right, .comma, .semicolon, .colon, .keyword_const, .keyword_pub => structure_color,
                else => if (self.mode == .Parens) style.comment_color else style.keyword_color,
            };
            std.mem.set(u.Color, colors[colors_start..colors_end], color);
        }
    }

    pub fn format(self: State, source: []const u8) ?[]const u8 {
        const source_z = self.allocator.dupeZ(u8, source) catch u.oom();
        defer self.allocator.free(source_z);
        var tree = std.zig.Ast.parse(self.allocator, source_z, .zig) catch u.oom();
        if (tree.errors.len > 0) return null;
        defer tree.deinit(self.allocator);
        return tree.render(self.allocator) catch u.oom();
    }

    pub fn getAddedIndent(self: State, token_ix: usize) usize {
        const token = self.tokens[token_ix];
        return switch (token) {
            .l_paren, .l_brace, .l_bracket => 4,
            else => 0,
        };
    }
};
