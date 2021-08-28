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
                for (colors) |*color| color.* = style.comment_color;
                const source_z = std.mem.dupeZ(allocator, u8, source[range[0]..range[1]]) catch oom();
                defer allocator.free(source_z);
                var tokenizer = std.zig.Tokenizer.init(source_z);
                while (true) {
                    const token = tokenizer.next();
                    switch (token.tag) {
                        .eof => break,
                        .doc_comment, .container_doc_comment => {},
                        .identifier => {
                            const highlight_color = highlightColor(tokenizer.buffer[token.loc.start..token.loc.end]);
                            for (colors[token.loc.start..token.loc.end]) |*color|
                                color.* = highlight_color;
                        },
                        else => {
                            for (colors[token.loc.start..token.loc.end]) |*color|
                                color.* = style.keyword_color;
                        },
                    }
                }
            },
            .Imp => {
                for (colors) |*color| color.* = style.comment_color;
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
                while (true) {
                    const start = parser.position;
                    if (parser.nextTokenMaybe()) |maybe_token| {
                        if (maybe_token) |token| {
                            switch (token) {
                                .EOF => break,
                                .None, .Some, .Number, .Text, .Name, .When, .Fix, .Reduce, .Enumerate => {
                                    const highlight_color = highlightColor(parser.source[start..parser.position]);
                                    for (colors[start..parser.position]) |*color|
                                        color.* = highlight_color;
                                },
                                else => {
                                    for (colors[start..parser.position]) |*color|
                                        color.* = style.keyword_color;
                                },
                            }
                        }
                    } else |err| {
                        if (err == error.OutOfMemory) oom();
                        parser.position += 1;
                    }
                }
            },
            else => {
                for (colors) |*color| color.* = style.text_color;
            },
        }
        return colors;
    }

    fn highlightColor(ident: []const u8) Color {
        const hash = meta.deepHash(ident);
        return Color{
            .r = @intCast(u8, 192 + (meta.deepHash([2]u64{ hash, 0 }) % 64)),
            .g = @intCast(u8, 192 + (meta.deepHash([2]u64{ hash, 1 }) % 64)),
            .b = @intCast(u8, 192 + (meta.deepHash([2]u64{ hash, 2 }) % 64)),
            .a = 255,
        };
    }
};
