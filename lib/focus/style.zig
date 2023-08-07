const std = @import("std");
const focus = @import("../focus.zig");
const u = focus.util;
const c = focus.util.c;

pub const background_color = u.Color.hsla(0, 0.0, 0.2, 1.0);
pub const fade_color = u.Color.hsla(0, 0.0, 0.2, 0.4);
pub const status_background_color = u.Color.hsla(0, 0.0, 0.1, 1.0);
pub const text_color = u.Color.hsla(0, 0.0, 0.9, 1.0);
pub const highlight_color = u.Color.hsla(0, 0.0, 0.9, 0.3);
pub const keyword_color = text_color;
pub const comment_color = u.Color.hsla(0, 0.0, 0.6, 1.0);
pub const multi_cursor_color = u.Color.hsla(150, 1.0, 0.5, 1.0);
pub const paren_match_color = u.Color.hsla(150, 1.0, 0.5, 0.3);

pub fn identColor(ident: []const u8) u.Color {
    const hash = @bitReverse(u.deepHash(ident));
    return u.Color.hsla(
        @floatFromInt(hash % 359),
        1.0,
        0.8,
        1.0,
    );
}

pub fn parenColor(level: usize) u.Color {
    const hash = @bitReverse(u.deepHash(level));
    return u.Color.hsla(
        @floatFromInt(hash % 359),
        1.0,
        0.8,
        1.0,
    );
}

pub const emphasisRed = u.Color.hsla(0, 1.0, 0.5, 1.0);
pub const emphasisOrange = u.Color.hsla(30, 1.0, 0.5, 1.0);
pub const emphasisGreen = u.Color.hsla(120, 1.0, 0.5, 1.0);
