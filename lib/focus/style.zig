const focus = @import("../focus.zig");
usingnamespace focus.common;

pub const background_color = Color.hsla(0, 0.0, 0.2, 1.0);
pub const fade_color = Color.hsla(0, 0.0, 0.2, 0.4);
pub const status_background_color = Color.hsla(0, 0.0, 0.1, 1.0);
pub const text_color = Color.hsla(0, 0.0, 0.9, 1.0);
pub const highlight_color = Color.hsla(0, 0.0, 0.9, 0.3);
pub const keyword_color = text_color;
pub const comment_color = Color.hsla(0, 0.0, 0.6, 1.0);
pub const error_text_color = Color.hsla(0, 1.0, 0.5, 1.0);
pub const eval_text_color = Color.hsla(180, 1.0, 0.5, 1.0);
pub const multi_cursor_color = Color.hsla(150, 1.0, 0.5, 1.0);
pub const paren_match_color = Color.hsla(150, 1.0, 0.5, 0.3);

pub fn identColor(ident: []const u8) Color {
    const hash = @bitReverse(u64, focus.meta.deepHash(ident));
    return Color.hsla(
        @intToFloat(f64, hash % 359),
        1.0,
        0.8,
        1.0,
    );
}

pub const emphasisColor = Color.hsla(0, 1.0, 0.5, 1.0);
