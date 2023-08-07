const std = @import("std");
const focus = @import("../../focus.zig");
const u = focus.util;
const c = focus.util.c;
const style = focus.style;
const language = focus.language;

pub const State = struct {
    allocator: u.Allocator,
    comment_string: []const u8,
    tokens: []const Token,
    token_ranges: []const [2]usize,
    paren_levels: []const usize,
    paren_parents: []const ?usize,
    paren_matches: []const ?usize,
    mode: enum {
        Normal,
        Parens,
    },

    pub fn init(allocator: u.Allocator, comment_string: []const u8, source: []const u8) State {
        var tokens = u.ArrayList(Token).init(allocator);
        var token_ranges = u.ArrayList([2]usize).init(allocator);
        var tokenizer = Tokenizer.init(comment_string, source);
        while (true) {
            const start = tokenizer.pos;
            const token = tokenizer.next();
            const end = tokenizer.pos;
            if (token == .eof) break;
            tokens.append(token) catch u.oom();
            token_ranges.append(.{ start, end }) catch u.oom();
        }

        const paren_levels = allocator.alloc(usize, tokens.items.len) catch u.oom();
        @memset(paren_levels, 0);
        const paren_parents = allocator.alloc(?usize, tokens.items.len) catch u.oom();
        @memset(paren_parents, null);
        const paren_matches = allocator.alloc(?usize, tokens.items.len) catch u.oom();
        @memset(paren_matches, null);
        var paren_match_stack = u.ArrayList(usize).init(allocator);
        for (tokens.items, 0..) |token, ix| {
            switch (token) {
                .close_paren, .close_bracket, .close_brace => {
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
                .open_paren, .open_bracket, .open_brace => {
                    paren_match_stack.append(ix) catch u.oom();
                },
                else => {},
            }
        }

        return .{
            .allocator = allocator,
            .comment_string = comment_string,
            .tokens = tokens.toOwnedSlice() catch u.oom(),
            .token_ranges = token_ranges.toOwnedSlice() catch u.oom(),
            .paren_levels = paren_levels,
            .paren_parents = paren_parents,
            .paren_matches = paren_matches,
            .mode = .Normal,
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
        const comment_string = self.comment_string;
        const mode = self.mode;
        self.deinit();
        self.* = State.init(allocator, comment_string, source);
        self.mode = mode;
    }

    pub fn toggleMode(self: *State) void {
        self.mode = switch (self.mode) {
            .Normal => .Parens,
            .Parens => .Normal,
        };
    }

    pub fn highlight(self: State, source: []const u8, range: [2]usize, colors: []u.Color) void {
        @memset(colors, style.comment_color);
        for (self.token_ranges, 0..) |token_range, i| {
            const source_start = token_range[0];
            const source_end = token_range[1];
            if (source_end < range[0] or source_start > range[1]) continue;
            const colors_start = if (source_start > range[0]) source_start - range[0] else 0;
            const colors_end = if (source_end > range[1]) range[1] - range[0] else source_end - range[0];
            const token = self.tokens[i];
            const color = switch (token) {
                .identifier => if (self.mode == .Parens) style.comment_color else style.identColor(source[source_start..source_end]),
                .comment, .whitespace => if (self.mode == .Parens) style.comment_color else style.comment_color,
                .open_paren, .open_bracket, .open_brace, .close_paren, .close_bracket, .close_brace => color: {
                    var is_good_match = false;
                    if (self.paren_matches[i]) |matching_ix| {
                        const matching_token = self.tokens[matching_ix];
                        is_good_match = switch (token) {
                            .open_paren => matching_token == .close_paren,
                            .open_bracket => matching_token == .close_bracket,
                            .open_brace => matching_token == .close_brace,
                            .close_paren => matching_token == .open_paren,
                            .close_bracket => matching_token == .open_bracket,
                            .close_brace => matching_token == .open_brace,
                            else => unreachable,
                        };
                    }
                    break :color if (is_good_match)
                        if (self.mode == .Parens)
                            style.parenColor(self.paren_levels[i])
                        else
                            style.comment_color
                    else
                        style.emphasisRed;
                },
                else => if (self.mode == .Parens) style.comment_color else style.keyword_color,
            };
            @memset(colors[colors_start..colors_end], color);
        }
    }

    pub fn getAddedIndent(self: State, token_ix: usize) usize {
        const token = self.tokens[token_ix];
        return switch (token) {
            .open_paren, .open_brace, .open_bracket => 4,
            else => 0,
        };
    }
};

pub const Token = enum {
    identifier,
    number,
    string,
    comment,
    whitespace,
    open_paren,
    close_paren,
    open_brace,
    close_brace,
    open_bracket,
    close_bracket,
    unknown,
    eof,
};

const TokenizerState = enum {
    start,
    identifier,
    number,
    string,
    string_escape,
    comment,
    whitespace,
};

pub const Tokenizer = struct {
    comment_string: []const u8,
    source: []const u8,
    pos: usize,

    pub fn init(comment_string: []const u8, source: []const u8) Tokenizer {
        return .{
            .comment_string = comment_string,
            .source = source,
            .pos = 0,
        };
    }

    pub fn next(self: *Tokenizer) Token {
        var state = TokenizerState.start;
        var quote_char: ?u8 = null;
        const source_len = self.source.len;
        while (true) {
            if (state == .start and
                self.pos < source_len and
                source_len - self.pos > self.comment_string.len and
                u.deepEqual(self.source[self.pos .. self.pos + self.comment_string.len], self.comment_string))
            {
                state = .comment;
                self.pos += self.comment_string.len;
                continue;
            }

            const char = if (self.pos < source_len) self.source[self.pos] else 0;
            self.pos += 1;
            switch (state) {
                .start => switch (char) {
                    0 => {
                        self.pos -= 1;
                        return .eof;
                    },
                    'a'...'z', 'A'...'Z' => state = .identifier,
                    '-', '0'...'9' => state = .number,
                    '\'', '"' => {
                        quote_char = char;
                        state = .string;
                    },
                    ' ', '\r', '\n', '\t' => state = .whitespace,
                    '(' => return .open_paren,
                    ')' => return .close_paren,
                    '{' => return .open_brace,
                    '}' => return .close_brace,
                    '[' => return .open_bracket,
                    ']' => return .close_bracket,
                    else => return .unknown,
                },
                .identifier => switch (char) {
                    'a'...'z', 'A'...'Z', '0'...'9', '_', '-' => {},
                    else => {
                        self.pos -= 1;
                        return .identifier;
                    },
                },
                .number => switch (char) {
                    '0'...'9', 'a'...'z', 'A'...'Z', '.', '-' => state = .number,
                    else => {
                        self.pos -= 1;
                        return .number;
                    },
                },
                .string => switch (char) {
                    0 => {
                        self.pos -= 1;
                        return .unknown;
                    },
                    '\\' => state = .string_escape,
                    else => {
                        if (char == quote_char) return .string;
                    },
                },
                .string_escape => switch (char) {
                    0 => {
                        self.pos -= 1;
                        return .unknown;
                    },
                    else => state = .string,
                },
                .comment => switch (char) {
                    0, '\n' => {
                        self.pos -= 1;
                        return .comment;
                    },
                    else => {},
                },
                .whitespace => switch (char) {
                    ' ', '\r', '\t' => {},
                    else => {
                        self.pos -= 1;
                        return .whitespace;
                    },
                },
            }
        }
    }
};
