const std = @import("std");
const focus = @import("../../focus.zig");
const u = focus.util;
const c = focus.util.c;
const style = focus.style;

pub const State = struct {
    allocator: u.Allocator,
    tokens: []const std.zig.Token.Tag,
    token_ranges: []const [2]usize,

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
        return .{
            .allocator = allocator,
            .tokens = tokens.toOwnedSlice(),
            .token_ranges = token_ranges.toOwnedSlice(),
        };
    }

    pub fn deinit(self: *State) void {
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
        self.deinit();
        self.* = State.init(allocator, source);
    }

    pub fn highlight(self: State, source: []const u8, range: [2]usize, colors: []u.Color) void {
        std.mem.set(u.Color, colors, style.comment_color);
        for (self.token_ranges) |token_range, i| {
            const source_start = token_range[0];
            const source_end = token_range[1];
            if (source_end < range[0] or source_start > range[1]) continue;
            const colors_start = if (source_start > range[0]) source_start - range[0] else 0;
            const colors_end = if (source_end > range[1]) range[1] - range[0] else source_end - range[0];
            const color = switch (self.tokens[i]) {
                .doc_comment, .container_doc_comment => style.comment_color,
                .identifier, .builtin, .integer_literal, .float_literal => style.identColor(source[source_start..source_end]),
                .keyword_try, .keyword_catch, .keyword_error => style.emphasisRed,
                .keyword_defer, .keyword_errdefer => style.emphasisOrange,
                .keyword_break, .keyword_continue, .keyword_return => style.emphasisGreen,
                else => style.keyword_color,
            };
            std.mem.set(u.Color, colors[colors_start..colors_end], color);
        }
    }

    pub fn format(self: State, source: []const u8) ?[]const u8 {
        const source_z = self.allocator.dupeZ(u8, source) catch u.oom();
        defer self.allocator.free(source_z);
        var tree = std.zig.parse(self.allocator, source_z) catch u.oom();
        if (tree.errors.len > 0) return null;
        defer tree.deinit(self.allocator);
        return tree.render(self.allocator) catch u.oom();
    }
};
