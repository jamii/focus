const std = @import("std");
const focus = @import("../../focus.zig");
const u = focus.util;
const c = focus.util.c;
const style = focus.style;
const Language = focus.Language;

pub const State = struct {
    allocator: u.Allocator,
    tokens: u.ArrayList(std.zig.Token.Tag),
    token_ranges: u.ArrayList([2]usize),
    paren_levels: u.ArrayList(usize),
    paren_parents: u.ArrayList(?usize),
    paren_matches: u.ArrayList(?usize),
    paren_match_stack: u.ArrayList(usize),
    mode: enum {
        Normal,
        NoStructure,
        Parens,
    },
    squigglies: u.ArrayList(Language.Squiggly),

    pub fn init(allocator: u.Allocator, source: []const u8) State {
        var state: State = .{
            .allocator = allocator,
            .tokens = u.ArrayList(std.zig.Token.Tag).init(allocator),
            .token_ranges = u.ArrayList([2]usize).init(allocator),
            .paren_levels = u.ArrayList(usize).init(allocator),
            .paren_parents = u.ArrayList(?usize).init(allocator),
            .paren_matches = u.ArrayList(?usize).init(allocator),
            .paren_match_stack = u.ArrayList(usize).init(allocator),
            .mode = .NoStructure,
            .squigglies = u.ArrayList(Language.Squiggly).init(allocator),
        };
        state.reset(source);
        return state;
    }

    pub fn deinit(self: *State) void {
        self.squigglies.deinit();
        self.paren_matches.deinit();
        self.paren_parents.deinit();
        self.paren_levels.deinit();
        self.token_ranges.deinit();
        self.tokens.deinit();
        self.* = undefined;
    }

    fn reset(self: *State, source: []const u8) void {
        self.squigglies.shrinkRetainingCapacity(0);
        self.paren_match_stack.shrinkRetainingCapacity(0);
        self.paren_matches.shrinkRetainingCapacity(0);
        self.paren_parents.shrinkRetainingCapacity(0);
        self.paren_levels.shrinkRetainingCapacity(0);
        self.token_ranges.shrinkRetainingCapacity(0);
        self.tokens.shrinkRetainingCapacity(0);

        // TODO Can we avoid this allocation?
        const source_z = self.allocator.dupeZ(u8, source) catch u.oom();
        defer self.allocator.free(source_z);

        var tokenizer = std.zig.Tokenizer.init(source_z);
        while (true) {
            const token = tokenizer.next();
            if (token.tag == .eof) break;
            self.tokens.append(token.tag) catch u.oom();
            self.token_ranges.append(.{ token.loc.start, token.loc.end }) catch u.oom();
        }

        self.paren_levels.appendNTimes(0, self.tokens.items.len) catch u.oom();
        self.paren_parents.appendNTimes(null, self.tokens.items.len) catch u.oom();
        self.paren_matches.appendNTimes(null, self.tokens.items.len) catch u.oom();

        for (self.tokens.items, 0..) |token, ix| {
            switch (token) {
                .r_paren, .r_brace, .r_bracket => {
                    if (self.paren_match_stack.pop()) |matching_ix| {
                        self.paren_matches.items[ix] = matching_ix;
                        self.paren_matches.items[matching_ix] = ix;
                    }
                },
                else => {},
            }
            if (self.paren_match_stack.items.len > 0)
                self.paren_parents.items[ix] = self.paren_match_stack.items[self.paren_match_stack.items.len - 1];
            self.paren_levels.items[ix] = self.paren_match_stack.items.len;
            switch (token) {
                .l_paren, .l_brace, .l_bracket => {
                    self.paren_match_stack.append(ix) catch u.oom();
                },
                else => {},
            }
        }

        // Disabled long line warning.
        //
        //{
        //    var line_start: usize = 0;
        //    while (line_start < source.len) {
        //        var line_end = line_start;
        //        while (line_end < source.len and source[line_end] != '\n') : (line_end += 1) {}
        //        if (line_end - line_start >= 100)
        //            squigglies.append(.{
        //                .color = style.emphasisOrange,
        //                .range = .{ line_start + 99, line_end },
        //            }) catch u.oom();
        //        line_start = line_end + 1;
        //    }
        //}
    }

    pub fn updateBeforeChange(self: *State, source: []const u8, delete_range: [2]usize) void {
        _ = self;
        _ = source;
        _ = delete_range;
    }

    pub fn updateAfterChange(self: *State, source: []const u8, insert_range: [2]usize) void {
        _ = insert_range;
        self.reset(source);
    }

    pub fn toggleMode(self: *State) void {
        self.mode = switch (self.mode) {
            .Normal => .NoStructure,
            .NoStructure => .Parens,
            .Parens => .Normal,
        };
    }

    pub fn highlight(self: State, source: []const u8, range: [2]usize, colors: []u.Color) void {
        @memset(colors, style.comment_color);
        for (self.token_ranges.items, 0..) |token_range, i| {
            const source_start = token_range[0];
            const source_end = token_range[1];
            if (source_end < range[0] or source_start > range[1]) continue;
            const colors_start = if (source_start > range[0]) source_start - range[0] else 0;
            const colors_end = if (source_end > range[1]) range[1] - range[0] else source_end - range[0];
            const token = self.tokens.items[i];
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
                    if (self.paren_matches.items[i]) |matching_ix| {
                        const matching_token = self.tokens.items[matching_ix];
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
                            style.parenColor(self.paren_levels.items[i])
                        else
                            structure_color
                    else
                        style.emphasisRed;
                },
                .pipe, .equal_angle_bracket_right, .comma, .semicolon, .colon, .keyword_const, .keyword_pub => structure_color,
                else => if (self.mode == .Parens) style.comment_color else style.keyword_color,
            };
            @memset(colors[colors_start..colors_end], color);
        }
    }

    pub fn format(self: State, frame_allocator: u.Allocator, source: []const u8) ?[]const u8 {
        // TODO Once zig syntax is stable, use Ast.render instead of shelling out

        var child_process = std.process.Child.init(
            &[_][]const u8{ "setsid", "zig", "fmt", "--stdin" },
            self.allocator,
        );

        child_process.stdin_behavior = .Pipe;
        child_process.stdout_behavior = .Pipe;
        child_process.stderr_behavior = .Pipe;

        child_process.spawn() catch |err|
            u.panic("Error spawning `zig fmt`: {}", .{err});

        child_process.stdin.?.writeAll(source) catch |err|
            u.panic("Error writing to `zig fmt` stdin: {}", .{err});
        child_process.stdin.?.close();
        child_process.stdin = null;

        var stdout = std.ArrayListUnmanaged(u8).empty;
        var stderr = std.ArrayListUnmanaged(u8).empty;

        child_process.collectOutput(frame_allocator, &stdout, &stderr, std.math.maxInt(usize)) catch |err|
            u.panic("Error collecting output from `zig fmt`: {}", .{err});

        const result = child_process.wait() catch |err|
            u.panic("Error waiting for `zig fmt`: {}", .{err});

        if (u.deepEqual(result, .{ .Exited = 0 })) {
            return stdout.toOwnedSlice(frame_allocator) catch u.oom();
        } else {
            u.warn("`zig fmt` failed: {s}", .{stderr.items});
            return null;
        }
    }

    pub fn getAddedIndent(self: State, token_ix: usize) usize {
        const token = self.tokens.items[token_ix];
        return switch (token) {
            .l_paren, .l_brace, .l_bracket => 4,
            else => 0,
        };
    }
};
