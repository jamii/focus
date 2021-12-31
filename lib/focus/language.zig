const std = @import("std");
const focus = @import("../focus.zig");
const u = focus.util;
const c = focus.util.c;
const style = focus.style;
const imp = @import("../../imp/lib/imp.zig");

pub const Language = enum {
    Zig,
    Java,
    Shell,
    Julia,
    Javascript,
    Imp,
    Unknown,

    pub fn fromFilename(filename: []const u8) Language {
        // TODO writing this as `return if ...` causes a confusing compiler error
        if (std.mem.endsWith(u8, filename, ".zig"))
            return .Zig
        else if (std.mem.endsWith(u8, filename, ".java"))
            return .Java
        else if (std.mem.endsWith(u8, filename, ".sh"))
            return .Shell
        else if (std.mem.endsWith(u8, filename, ".jl"))
            return .Julia
        else if (std.mem.endsWith(u8, filename, ".js"))
            return .Javascript
        else if (std.mem.endsWith(u8, filename, ".imp"))
            return .Imp
        else
            return .Unknown;
    }

    pub fn commentString(self: Language) ?[]const u8 {
        return switch (self) {
            .Zig, .Java, .Javascript, .Imp => "//",
            .Shell, .Julia => "#",
            .Unknown => null,
        };
    }

    pub fn highlight(self: Language, allocator: u.Allocator, source: []const u8, range: [2]usize) []const u.Color {
        const colors = allocator.alloc(u.Color, range[1] - range[0]) catch u.oom();
        switch (self) {
            .Zig => {
                const source_z = allocator.dupeZ(u8, source[range[0]..range[1]]) catch u.oom();
                defer allocator.free(source_z);
                var tokenizer = std.zig.Tokenizer.init(source_z);
                std.mem.set(u.Color, colors, style.comment_color);
                while (true) {
                    const token = tokenizer.next();
                    switch (token.tag) {
                        .eof => break,
                        .doc_comment, .container_doc_comment => {},
                        .identifier, .builtin, .integer_literal, .float_literal => std.mem.set(
                            u.Color,
                            colors[token.loc.start..token.loc.end],
                            style.identColor(tokenizer.buffer[token.loc.start..token.loc.end]),
                        ),
                        .keyword_try, .keyword_catch => std.mem.set(
                            u.Color,
                            colors[token.loc.start..token.loc.end],
                            style.emphasisRed,
                        ),
                        .keyword_defer, .keyword_errdefer => std.mem.set(
                            u.Color,
                            colors[token.loc.start..token.loc.end],
                            style.emphasisOrange,
                        ),
                        .keyword_break, .keyword_continue, .keyword_return => std.mem.set(
                            u.Color,
                            colors[token.loc.start..token.loc.end],
                            style.emphasisGreen,
                        ),
                        else => std.mem.set(
                            u.Color,
                            colors[token.loc.start..token.loc.end],
                            style.keyword_color,
                        ),
                    }
                }
            },
            .Imp => {
                var arena = u.ArenaAllocator.init(allocator);
                defer arena.deinit();
                var error_info: ?imp.lang.pass.parse.ErrorInfo = null;
                var parser = imp.lang.pass.parse.Parser{
                    .arena = &arena,
                    .source = source[range[0]..range[1]],
                    .exprs = u.ArrayList(imp.lang.repr.syntax.Expr).init(arena.allocator()),
                    .from_source = u.ArrayList([2]usize).init(arena.allocator()),
                    .position = 0,
                    .error_info = &error_info,
                };
                std.mem.set(u.Color, colors, style.comment_color);
                while (true) {
                    const start = parser.position;
                    if (parser.nextTokenMaybe()) |maybe_token| {
                        if (maybe_token) |token| {
                            switch (token) {
                                .EOF => break,
                                .Number, .Text, .Name => std.mem.set(
                                    u.Color,
                                    colors[start..parser.position],
                                    style.identColor(parser.source[start..parser.position]),
                                ),
                                else => std.mem.set(
                                    u.Color,
                                    colors[start..parser.position],
                                    style.keyword_color,
                                ),
                            }
                        }
                    } else |err| {
                        if (err == error.OutOfMemory) u.oom();
                        parser.position += 1;
                    }
                }
            },
            else => {
                std.mem.set(u.Color, colors, style.text_color);
            },
        }
        return colors;
    }
};
