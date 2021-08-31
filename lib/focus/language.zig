const focus = @import("../focus.zig");
usingnamespace focus.common;
const style = focus.style;
const meta = focus.meta;
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

    pub fn highlight(self: Language, allocator: *Allocator, source: []const u8, range: [2]usize) []const Color {
        _ = source;
        const colors = allocator.alloc(Color, range[1] - range[0]) catch oom();
        switch (self) {
            .Zig => {
                const source_z = std.mem.dupeZ(allocator, u8, source[range[0]..range[1]]) catch oom();
                defer allocator.free(source_z);
                var tokenizer = std.zig.Tokenizer.init(source_z);
                std.mem.set(Color, colors, style.comment_color);
                while (true) {
                    const token = tokenizer.next();
                    switch (token.tag) {
                        .eof => break,
                        .doc_comment, .container_doc_comment => {},
                        .identifier, .builtin, .integer_literal, .float_literal => std.mem.set(
                            Color,
                            colors[token.loc.start..token.loc.end],
                            style.identColor(tokenizer.buffer[token.loc.start..token.loc.end]),
                        ),
                        .keyword_try, .keyword_catch => std.mem.set(
                            Color,
                            colors[token.loc.start..token.loc.end],
                            style.emphasisColor,
                        ),
                        else => std.mem.set(
                            Color,
                            colors[token.loc.start..token.loc.end],
                            style.keyword_color,
                        ),
                    }
                }
            },
            .Imp => {
                var arena = ArenaAllocator.init(allocator);
                defer arena.deinit();
                var store = imp.lang.Store.init(&arena);
                var error_info: ?imp.lang.pass.parse.ErrorInfo = null;
                var parser = imp.lang.pass.parse.Parser{
                    .store = &store,
                    .source = source[range[0]..range[1]],
                    .position = 0,
                    .error_info = &error_info,
                };
                std.mem.set(Color, colors, style.comment_color);
                while (true) {
                    const start = parser.position;
                    if (parser.nextTokenMaybe()) |maybe_token| {
                        if (maybe_token) |token| {
                            switch (token) {
                                .EOF => break,
                                .None, .Some, .Number, .Text, .Name, .When, .Fix, .Reduce, .Enumerate => std.mem.set(
                                    Color,
                                    colors[start..parser.position],
                                    style.identColor(parser.source[start..parser.position]),
                                ),
                                else => std.mem.set(
                                    Color,
                                    colors[start..parser.position],
                                    style.keyword_color,
                                ),
                            }
                        }
                    } else |err| {
                        if (err == error.OutOfMemory) oom();
                        parser.position += 1;
                    }
                }
            },
            else => {
                std.mem.set(Color, colors, style.text_color);
            },
        }
        return colors;
    }
};
