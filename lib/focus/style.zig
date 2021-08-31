const focus = @import("../focus.zig");
usingnamespace focus.common;

pub const background_color = Color{ .r = 0x2e, .g = 0x34, .b = 0x36, .a = 255 };
pub const fade_color = Color{ .r = 0x2e, .g = 0x34, .b = 0x36, .a = 100 };
pub const status_background_color = Color{ .r = @divTrunc(0x2e, 2), .g = @divTrunc(0x34, 2), .b = @divTrunc(0x36, 2), .a = 255 };
pub const text_color = Color{ .r = 0xee, .g = 0xee, .b = 0xec, .a = 255 };
pub const keyword_color = Color{ .r = 0xff, .g = 0xff, .b = 0xff, .a = 255 };
pub const comment_color = Color{ .r = 0xaa, .g = 0xaa, .b = 0xaa, .a = 255 };
pub const error_text_color = Color{ .r = 0xff, .g = 0x00, .b = 0x00, .a = 255 };
pub const highlight_color = Color{ .r = 0xee, .g = 0xee, .b = 0xec, .a = 100 };
pub const error_highlight_color = Color{ .r = 0xff, .g = 0x00, .b = 0x00, .a = 100 };
pub const multi_cursor_color = Color{ .r = 0x7a, .g = 0xa6, .b = 0xda, .a = 255 };
pub const paren_match_color = Color{ .r = 0x7a, .g = 0xa6, .b = 0xda, .a = 100 };

pub fn highlightColor(ident: []const u8) Color {
    const hash = focus.meta.deepHash(.{ @intCast(u64, 7919), ident });
    const saturation = [3]f64{ 0.4, 0.6, 0.8 };
    return Color.hsl(
        @intToFloat(f64, hash % 360),
        saturation[@divTrunc(hash, 360) % 3],
        0.8,
    );
}
