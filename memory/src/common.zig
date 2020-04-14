pub usingnamespace @cImport({
    @cInclude("SDL2/SDL.h");
    @cInclude("SDL2/SDL_opengles2.h");
    // these have to be the same as in nuklear.c
    // apart from NK_IMPLEMENTATION
    @cDefine("NK_INCLUDE_FIXED_TYPES", {});
    @cDefine("NK_INCLUDE_STANDARD_IO", {});
    @cDefine("NK_INCLUDE_STANDARD_VARARGS", {});
    @cDefine("NK_INCLUDE_DEFAULT_ALLOCATOR", {});
    @cDefine("NK_INCLUDE_VERTEX_BUFFER_OUTPUT", {});
    @cDefine("NK_INCLUDE_FONT_BAKING", {});
    @cDefine("NK_INCLUDE_DEFAULT_FONT", {});
    @cInclude("nuklear.h");
});

pub const std = @import("std");
pub const warn = std.debug.warn;
pub const assert = std.debug.assert;
pub const Allocator = std.mem.Allocator;
