const std = @import("std");
const focus = @import("../../focus.zig");
const u = focus.util;
const c = focus.util.c;

// https://github.com/clojure/clojure/blob/master/src/jvm/clojure/lang/LispReader.java

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

pub const Tokenizer = struct {
    source: []const u8,
    pos: usize,

    pub fn init(source: []const u8) Tokenizer {
        return .{
            .source = source,
            .pos = 0,
        };
    }

    const State = enum {
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

    pub fn next(self: *Tokenizer) Token {
        var state: State = .start;
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
                    ' ', ',', '\r', '\n', '\t' => state = .whitespace,
                    '0'...'9' => state = .number,
                    'a'...'z', 'A'...'Z', '*', '+', '!', '_', '?', '<', '>', '=' => {
                        enough_chars = true;
                        state = .symbol;
                    },
                    '-' => state = .minus,
                    ':' => state = .keyword,
                    else => return .err,
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
                .character => if (!enough_chars)
                    switch (char) {
                        0, ' ', ',', '\r', '\n', '\t' => {
                            self.pos -= 1;
                            return .err;
                        },
                        'u' => state = .character_unicode_escape,
                        'o' => state = .character_octal_escape,
                        // TODO handle unicode
                        else => enough_chars = true,
                    }
                else switch (char) {
                    0, ' ', ',', '\r', '\n', '\t', '(', ')', '[', ']', '{', '}', '"' => {
                        self.pos -= 1;
                        return .character;
                    },
                    else => {},
                },
                .character_unicode_escape => switch (char) {
                    0, ' ', ',', '\r', '\n', '\t', '(', ')', '[', ']', '{', '}', '"' => {
                        // TODO validate character
                        self.pos -= 1;
                        return if (enough_chars) .character_unicode_escape else .character;
                    },
                    else => enough_chars = true,
                },
                .character_octal_escape => switch (char) {
                    0, ' ', ',', '\r', '\n', '\t', '(', ')', '[', ']', '{', '}', '"' => {
                        // TODO validate character
                        self.pos -= 1;
                        return if (enough_chars) .character_octal_escape else .character;
                    },
                    else => enough_chars = true,
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
                    '#' => {
                        symbol_token = .symbolic_value;
                        state = .symbol;
                    },
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
                    'a'...'z', 'A'...'Z', '*', '+', '>' => {
                        symbol_token = .tag;
                        enough_chars = true;
                        state = .symbol;
                    },
                    '?' => state = .reader_conditional,
                    ':' => state = .namespaced_map,
                    else => return .err,
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
                .whitespace => switch (char) {
                    ' ', ',', '\r', '\t' => {},
                    else => {
                        self.pos -= 1;
                        return .whitespace;
                    },
                },
                .number => switch (char) {
                    // TODO validate numbers
                    '0'...'9', 'a'...'z', 'A'...'Z', '.', '-', '/' => state = .number,
                    else => {
                        self.pos -= 1;
                        return .number;
                    },
                },
                .symbol => switch (char) {
                    // TODO validate symbols
                    'a'...'z', 'A'...'Z', '*', '+', '!', '_', '?', '<', '>', '=', '0'...'9', '\'', '-', '.', '$', '/', '#' => enough_chars = true,
                    else => {
                        self.pos -= 1;
                        return if (enough_chars) symbol_token else .err;
                    },
                },
                .minus => switch (char) {
                    'a'...'z', 'A'...'Z', '*', '+', '!', '_', '?', '<', '>', '=', '\'', '-', '.', '$', '/', '#' => state = .symbol,
                    '0'...'9' => state = .number,
                    else => return .err,
                },
                .keyword => switch (char) {
                    0 => {
                        self.pos -= 1;
                        return if (enough_chars) .keyword else .err;
                    },
                    ' ', ',', '\r', '\n', '\t', '(', ')', '[', ']', '{', '}', '"' => {
                        // TODO validate keywords
                        self.pos -= 1;
                        return .keyword;
                    },
                    else => enough_chars = true,
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
    try testTokenize("##", &.{.err});
    try testTokenize("##NaN", &.{.symbolic_value});
    try testTokenize("##-NaN", &.{.symbolic_value});
    try testTokenize("##Inf", &.{.symbolic_value});
    try testTokenize("##-Inf", &.{.symbolic_value});
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

    // these should fail validation
    try testTokenize("##foo", &.{.symbolic_value});
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

test "real code" {}

//[\u0043"foo"]
//[\C "foo"]

//[\u0043%4]

//[\u0043#foo]

//[\u0043#foo[]]

//user=> [\u0043{}]
//[\C {}]

//user=> [\u0043#{}]

//user=> #(%20)
//#object[user$eval265$fn__266 0x1c025cb "user$eval265$fn__266@1c025cb"]
//user=> #(%)
//#object[user$eval271$fn__272 0x5ecba515 "user$eval271$fn__272@5ecba515"]
//user=> #(%1)
//#object[user$eval277$fn__278 0x60723d6a "user$eval277$fn__278@60723d6a"]
//user=> #(%1&)

//user=> [##NaN1 ]
//Syntax error reading source at (REPL:53:8).
//Unknown symbolic value: ##NaN1
//Syntax error reading source at (REPL:53:10).
//Unmatched delimiter: ]
//user=> [##NaN 1]
//[##NaN 1]
//user=> [##NaN"foo"]
//[##NaN "foo"]

//user=> #? () ()
//()
//user=> [#? () ()]
//[()]
//user=>

//user=> (def -a1)
//#'user/-a1
//user=> (def -1a)
//Syntax error reading source at (REPL:78:9).
//Invalid number: -1a
//Syntax error reading source at (REPL:78:10).
//Unmatched delimiter: )

//(def ab'cd)

//user=> (def -'foo)
//#'user/-'foo

//user=> '-/
//Syntax error reading source at (REPL:90:0).
//Invalid token: -/
//user=> '-//
//-//

//user=> #>foo []
//Syntax error reading source at (REPL:91:9).
//No reader function for tag >foo

//user=> 32r123Zz
//34922
//user=> 32N1
//Syntax error reading source at (REPL:99:0).
//Invalid number: 32N1

//user=> 32r123N1
//1117921

//user=> :1
//:1
//user=> :/
//:/
//user=> :$
//:$
//user=> :.
//:.
//user=> :#
//:#
//user=> :#
//user=> :Ã
//:Ã

//[:as[]]

//[\o5[]]

//user=> #'#inst "2022-06-30T15:45:28.733-00:00"
//Syntax error (ClassCastException) compiling var at (REPL:0:0).
//class java.util.Date cannot be cast to class clojure.lang.Symbol (java.util.Date is in module java.base of loader 'bootstrap'; clojure.lang.Symbol is in unnamed module of loader 'app')

//user=> ##inst "2022-06-30T15:45:28.733-00:00"
//Syntax error reading source at (REPL:9:7).
//Unknown symbolic value: ##inst
//"2022-06-30T15:45:28.733-00:00"
