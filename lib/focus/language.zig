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

    pub fn commentString(language: Language) ?[]const u8 {
        return switch (language) {
            .Zig, .Java, .Javascript, .Imp => "//",
            .Shell, .Julia => "#",
            .Unknown => null,
        };
    }
};
