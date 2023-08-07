const std = @import("std");
const focus = @import("../../focus.zig");
const u = focus.util;
const c = focus.util.c;
const style = focus.style;
const language = focus.language;

pub const State = struct {
    allocator: u.Allocator,
    tokens: []const Token,
    token_ranges: []const [2]usize,
    paren_levels: []const usize,
    paren_parents: []const ?usize,
    paren_matches: []const ?usize,
    mode: enum {
        Normal,
        Parens,
    },

    pub fn init(allocator: u.Allocator, source: []const u8) State {
        var tokens = u.ArrayList(Token).init(allocator);
        var token_ranges = u.ArrayList([2]usize).init(allocator);
        var tokenizer = Tokenizer.init(source);
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
                .close_list, .close_vec, .close_map => {
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
                .open_list, .open_vec, .open_map, .open_fun, .open_set => {
                    paren_match_stack.append(ix) catch u.oom();
                },
                else => {},
            }
        }

        return .{
            .allocator = allocator,
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
        const mode = self.mode;
        self.deinit();
        self.* = State.init(allocator, source);
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
                .err => style.emphasisRed,
                .symbol, .keyword => if (self.mode == .Parens) style.comment_color else style.identColor(source[source_start..source_end]),
                .comment, .whitespace => style.comment_color,
                .open_list, .open_map, .open_vec, .open_set, .open_fun, .close_list, .close_map, .close_vec => color: {
                    var is_good_match = false;
                    if (self.paren_matches[i]) |matching_ix| {
                        const matching_token = self.tokens[matching_ix];
                        is_good_match = switch (token) {
                            .open_list, .open_fun => matching_token == .close_list,
                            .open_map, .open_set => matching_token == .close_map,
                            .open_vec => matching_token == .close_vec,
                            .close_list => matching_token == .open_list or matching_token == .open_fun,
                            .close_map => matching_token == .open_map or matching_token == .open_set,
                            .close_vec => matching_token == .open_vec,
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
            .open_list, .open_fun => 2,
            .open_vec, .open_map, .open_set => 1,
            else => 0,
        };
    }
};

pub const Token = enum {
    quote,
    deref,
    meta,
    syntax_quote,
    open_list,
    close_list,
    open_vec,
    close_vec,
    open_map,
    close_map,
    string,
    unquote_splice,
    unquote,
    comment,
    character,
    character_unicode_escape,
    character_octal_escape,
    arg,
    open_fun,
    open_set,
    eval,
    discard,
    unreadable,
    reader_conditional_splice,
    reader_conditional,
    namespaced_map_this,
    namespaced_map_alias,
    whitespace,
    number,
    symbol,
    tag,
    symbolic_value,
    var_symbol,
    regex,
    keyword,
    eof,
    err,
};

const TokenizerState = enum {
    start,
    string,
    string_escape,
    unquote,
    comment,
    character,
    character_unicode_escape,
    character_octal_escape,
    arg,
    dispatch,
    reader_conditional,
    namespaced_map,
    whitespace,
    number,
    symbol,
    minus,
    keyword,
    unreadable,
};

pub const Tokenizer = struct {
    source: []const u8,
    pos: usize,

    pub fn init(source: []const u8) Tokenizer {
        return .{
            .source = source,
            .pos = 0,
        };
    }

    inline fn is_whitespace(char: u8) bool {
        return switch (char) {
            ' ', ',', '\r', '\n', '\t' => true,
            else => false,
        };
    }

    inline fn is_terminator(char: u8) bool {
        return switch (char) {
            0, ' ', ',', '\r', '\n', '\t', '(', ')', '[', ']', '{', '}', '"' => true,
            else => false,
        };
    }

    inline fn is_symbol(char: u8) bool {
        return switch (char) {
            'a'...'z', 'A'...'Z', '&', '*', '+', '!', '_', '?', '<', '>', '=', '0'...'9', '\'', '-', '.', '$', '/', '#' => true,
            else => false,
        };
    }

    // https://github.com/clojure/clojure/blob/master/src/jvm/clojure/lang/LispReader.java
    pub fn next(self: *Tokenizer) Token {
        var state = TokenizerState.start;
        const source_len = self.source.len;
        var string_token: Token = .string;
        var symbol_token: Token = .symbol;
        var enough_chars = false;
        while (true) {
            const char = if (self.pos < source_len) self.source[self.pos] else 0;
            self.pos += 1;
            switch (state) {
                .start => switch (char) {
                    0 => {
                        self.pos -= 1;
                        return .eof;
                    },
                    '"' => state = .string,
                    ';' => state = .comment,
                    '\'' => return .quote,
                    '@' => return .deref,
                    '^' => return .meta,
                    '`' => return .syntax_quote,
                    '~' => state = .unquote,
                    '(' => return .open_list,
                    ')' => return .close_list,
                    '[' => return .open_vec,
                    ']' => return .close_vec,
                    '{' => return .open_map,
                    '}' => return .close_map,
                    '\\' => state = .character,
                    '%' => state = .arg,
                    '#' => state = .dispatch,
                    '0'...'9' => state = .number,
                    '-' => state = .minus,
                    ':' => state = .keyword,
                    else => {
                        if (is_whitespace(char)) {
                            state = .whitespace;
                        } else if (is_symbol(char)) {
                            enough_chars = true;
                            state = .symbol;
                        } else {
                            return .err;
                        }
                    },
                },
                .string => switch (char) {
                    0 => {
                        self.pos -= 1;
                        return .err;
                    },
                    '\\' => state = .string_escape,
                    '"' => return string_token,
                    else => {},
                },
                .string_escape => switch (char) {
                    0 => {
                        self.pos -= 1;
                        return .err;
                    },
                    // TODO validate \u unicode escape
                    't', 'b', 'n', 'r', 'f', '\'', '"', 'u', '\\' => state = .string,
                    // TODO handle unicode codepoints
                    else => return .err,
                },
                .unquote => switch (char) {
                    '@' => return .unquote_splice,
                    else => {
                        self.pos -= 1;
                        return .unquote;
                    },
                },
                .comment => switch (char) {
                    0, '\n' => {
                        self.pos -= 1;
                        return .comment;
                    },
                    else => {},
                },
                .character => {
                    if (!enough_chars) {
                        switch (char) {
                            'u' => state = .character_unicode_escape,
                            'o' => state = .character_octal_escape,
                            // TODO handle unicode
                            else => {
                                if (is_whitespace(char)) {
                                    self.pos -= 1;
                                    return .err;
                                } else {
                                    enough_chars = true;
                                }
                            },
                        }
                    } else if (is_terminator(char)) {
                        self.pos -= 1;
                        return .character;
                    } else {}
                },
                .character_unicode_escape => {
                    if (is_terminator(char)) {
                        // TODO validate character
                        self.pos -= 1;
                        return if (enough_chars) .character_unicode_escape else .character;
                    } else {
                        enough_chars = true;
                    }
                },
                .character_octal_escape => {
                    if (is_terminator(char)) {
                        // TODO validate character
                        self.pos -= 1;
                        return if (enough_chars) .character_octal_escape else .character;
                    } else {
                        enough_chars = true;
                    }
                },
                .arg => switch (char) {
                    '0'...'9' => enough_chars = true,
                    '&' => {
                        if (enough_chars) self.pos -= 1;
                        return .arg;
                    },
                    else => {
                        self.pos -= 1;
                        return .arg;
                    },
                },
                .dispatch => switch (char) {
                    0 => {
                        self.pos -= 1;
                        return .err;
                    },
                    '^' => return .meta,
                    '#' => return .symbolic_value,
                    '\'' => return .var_symbol,
                    '"' => {
                        string_token = .regex;
                        state = .string;
                    },
                    '(' => return .open_fun,
                    '{' => return .open_set,
                    '=' => return .eval,
                    '!' => state = .comment,
                    '<' => state = .unreadable,
                    '_' => return .discard,
                    '?' => state = .reader_conditional,
                    ':' => state = .namespaced_map,
                    else => {
                        if (is_symbol(char)) {
                            symbol_token = .tag;
                            enough_chars = true;
                            state = .symbol;
                        } else {
                            return .err;
                        }
                    },
                },
                // The syntax inside unreadable is undefined.
                // Clojure just panics.
                // But looking for matching '>' is often good enough to syntax highlight repl output.
                .unreadable => switch (char) {
                    0 => {
                        self.pos -= 1;
                        return .err;
                    },
                    '>' => return .unreadable,
                    else => {},
                },
                .reader_conditional => switch (char) {
                    '@' => return .reader_conditional_splice,
                    else => {
                        self.pos -= 1;
                        return .reader_conditional;
                    },
                },
                .namespaced_map => switch (char) {
                    ':' => return .namespaced_map_this,
                    else => {
                        self.pos -= 1;
                        return .namespaced_map_alias;
                    },
                },
                .whitespace => {
                    if (!is_whitespace(char)) {
                        self.pos -= 1;
                        return .whitespace;
                    }
                },
                .number => switch (char) {
                    // TODO validate numbers
                    '0'...'9', 'a'...'z', 'A'...'Z', '.', '-', '/' => state = .number,
                    else => {
                        self.pos -= 1;
                        return .number;
                    },
                },
                .symbol => {
                    if (is_symbol(char)) {
                        enough_chars = true;
                    } else {
                        self.pos -= 1;
                        return if (enough_chars) symbol_token else .err;
                    }
                },
                .minus => switch (char) {
                    '0'...'9' => state = .number,
                    else => {
                        if (is_symbol(char)) {
                            enough_chars = true;
                            state = .symbol;
                        } else if (is_terminator(char)) {
                            self.pos -= 1;
                            return .symbol;
                        } else {
                            return .err;
                        }
                    },
                },
                .keyword => switch (char) {
                    0 => {
                        self.pos -= 1;
                        return if (enough_chars) .keyword else .err;
                    },
                    else => {
                        if (is_terminator(char)) {
                            // TODO validate keywords
                            self.pos -= 1;
                            return .keyword;
                        } else {
                            enough_chars = true;
                        }
                    },
                },
            }
        }
    }
};

fn testTokenize(source: []const u8, expected_tokens: []const Token) !void {
    var tokenizer = Tokenizer.init(source);
    for (expected_tokens) |expected_token| {
        const token = tokenizer.next();
        try std.testing.expectEqual(expected_token, token);
    }
    const last_token = tokenizer.next();
    try std.testing.expectEqual(Token.eof, last_token);
    try std.testing.expectEqual(source.len, tokenizer.pos);
}

test "basic" {
    try testTokenize("'", &.{.quote});
    try testTokenize("@", &.{.deref});
    try testTokenize("^", &.{.meta});
    try testTokenize("`", &.{.syntax_quote});
    try testTokenize("()", &.{ .open_list, .close_list });
    try testTokenize("[]", &.{ .open_vec, .close_vec });
    try testTokenize("{}", &.{ .open_map, .close_map });
    try testTokenize("~", &.{.unquote});
    try testTokenize("~@", &.{.unquote_splice});
    try testTokenize("; foo", &.{.comment});
    try testTokenize(
        \\; foo
        \\foo
    , &.{ .comment, .whitespace, .symbol });
    try testTokenize("#()", &.{ .open_fun, .close_list });
    try testTokenize("#{}", &.{ .open_set, .close_map });
    try testTokenize("#=()", &.{ .eval, .open_list, .close_list });
}

test "characters" {
    try testTokenize(
        \\\a
    , &.{.character});
    try testTokenize(
        \\\u
    , &.{.character});
    try testTokenize(
        \\\o
    , &.{.character});
    try testTokenize(
        \\\u0042
    , &.{.character_unicode_escape});
    try testTokenize(
        \\\uaaaa
    , &.{.character_unicode_escape});
    try testTokenize(
        \\\o3
    , &.{.character_octal_escape});
    try testTokenize(
        \\\o33
    , &.{.character_octal_escape});
    try testTokenize(
        \\\o333
    , &.{.character_octal_escape});
    try testTokenize(
        \\\newline
    , &.{.character});

    try testTokenize(
        \\\ 
    , &.{ .err, .whitespace });

    // these should fail validation
    try testTokenize(
        \\\uaaa
    , &.{.character_unicode_escape});
    try testTokenize(
        \\\uaaaaa
    , &.{.character_unicode_escape});
    try testTokenize(
        \\\o3333
    , &.{.character_octal_escape});
    try testTokenize(
        \\\yomama
    , &.{.character});
    try testTokenize(
        \\\"foo
    , &.{.character});
}

test "strings" {
    try testTokenize(
        \\"foo""bar"
    , &.{ .string, .string });
    try testTokenize(
        \\"foo\""bar
    , &.{ .string, .symbol });
    try testTokenize(
        \\"\n\r\\"
    , &.{.string});

    try testTokenize(
        \\"\z"
    , &.{ .err, .err });

    // these should fail validation
    try testTokenize(
        \\"\u"
    , &.{.string});
    try testTokenize(
        \\"\u012"
    , &.{.string});
    try testTokenize(
        \\"\u01234"
    , &.{.string});
}

test "args" {
    try testTokenize("%", &.{.arg});
    try testTokenize("%18", &.{.arg});
    try testTokenize("%&", &.{.arg});
}

test "symbols" {
    try testTokenize("a", &.{.symbol});
    try testTokenize("-><-", &.{.symbol});
    try testTokenize("???", &.{.symbol});
    try testTokenize("foo.bar/bar", &.{.symbol});
    try testTokenize("java$has$dollars", &.{.symbol});
    try testTokenize("a##", &.{.symbol});
    try testTokenize("-a1", &.{.symbol});

    // these should fail validation
    try testTokenize("foo/bar.bar", &.{.symbol});
    try testTokenize("foo$bar/bar", &.{.symbol});
}

test "numbers" {
    try testTokenize("1", &.{.number});
    try testTokenize("3.14", &.{.number});
    try testTokenize("32N", &.{.number});
    try testTokenize("-32r123Zz", &.{.number});
    try testTokenize("1/2", &.{.number});
    try testTokenize("-1/2", &.{.number});

    // these should fail validation
    try testTokenize("32N1", &.{.number});
    try testTokenize("32r123Zz", &.{.number});
    try testTokenize("-1a", &.{.number});
    try testTokenize("1/2/3", &.{.number});
}

test "keywords" {
    try testTokenize(":foo", &.{.keyword});
    try testTokenize(":1", &.{.keyword});
    try testTokenize(":#-?", &.{.keyword});

    try testTokenize(":", &.{.err});
}

test "dispatch" {
    try testTokenize("#", &.{.err});
    try testTokenize("#^1", &.{ .meta, .number });
    try testTokenize("##", &.{.symbolic_value});
    try testTokenize("##NaN", &.{ .symbolic_value, .symbol });
    try testTokenize("##-NaN", &.{ .symbolic_value, .symbol });
    try testTokenize("##Inf", &.{ .symbolic_value, .symbol });
    try testTokenize("##-Inf", &.{ .symbolic_value, .symbol });
    try testTokenize("#'foo", &.{ .var_symbol, .symbol });
    try testTokenize("#'[]", &.{ .var_symbol, .open_vec, .close_vec }); // yeah, this parses in clojure
    try testTokenize(
        \\#""
    , &.{.regex});
    try testTokenize(
        \\#"\"foo\u0042"
    , &.{.regex});
    try testTokenize("#()", &.{ .open_fun, .close_list });
    try testTokenize("#{}", &.{ .open_set, .close_map });
    try testTokenize("#=(+ 1 1)", &.{ .eval, .open_list, .symbol, .whitespace, .number, .whitespace, .number, .close_list });
    try testTokenize("#! foo", &.{.comment});
    try testTokenize("#! foo", &.{.comment});
    try testTokenize(
        \\[1 #<Some (junk)
    , &.{ .open_vec, .number, .whitespace, .err });
    try testTokenize(
        \\[1 #<Some (junk)
        \\...
        \\> 3]
    , &.{ .open_vec, .number, .whitespace, .unreadable, .whitespace, .number, .close_vec });
    try testTokenize("#_1", &.{ .discard, .number });
    try testTokenize("#foo 1", &.{ .tag, .whitespace, .number });
    try testTokenize("#foo1", &.{.tag});
    try testTokenize(
        \\#foo1"bar"
    , &.{ .tag, .string });
    try testTokenize("#?()", &.{ .reader_conditional, .open_list, .close_list });
    try testTokenize("#::{}", &.{ .namespaced_map_this, .open_map, .close_map });
    try testTokenize("#::foo", &.{ .namespaced_map_this, .symbol });
    try testTokenize("#:foo{}", &.{ .namespaced_map_alias, .symbol, .open_map, .close_map });
}

test "boundaries" {
    try testTokenize("(def a[1])", &.{ .open_list, .symbol, .whitespace, .symbol, .open_vec, .number, .close_vec, .close_list });
    try testTokenize(
        \\[:#-?"foo"]
    , &.{ .open_vec, .keyword, .string, .close_vec });
    try testTokenize(
        \\[\u0043"foo"]
    , &.{ .open_vec, .character_unicode_escape, .string, .close_vec });
    try testTokenize(
        \\[\c"foo"]
    , &.{ .open_vec, .character, .string, .close_vec });
    try testTokenize(
        \\[\newline"foo"]
    , &.{ .open_vec, .character, .string, .close_vec });
    try testTokenize(
        \\[\([]]
    , &.{ .open_vec, .character, .open_vec, .close_vec, .close_vec });

    // these should fail validation
    try testTokenize(
        \\[\u0043#foo[]]
    , &.{ .open_vec, .character_unicode_escape, .open_vec, .close_vec, .close_vec });
}

test "real code" {
    try testTokenize("(- 1)", &.{ .open_list, .symbol, .whitespace, .number, .close_list });

    try testTokenize(
        \\(.startsWith(:uri request) "/out")
    , &.{ .open_list, .symbol, .open_list, .keyword, .whitespace, .symbol, .close_list, .whitespace, .string, .close_list });

    try testTokenize("#/ {}", &.{ .tag, .whitespace, .open_map, .close_map });
}
