const focus = @import("../focus.zig");
usingnamespace focus.common;
const style = focus.style;
const meta = focus.meta;

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
            else => {
                for (colors) |*color, i| color.* = style.text_color;
            },
        }
        return colors;
    }
};
