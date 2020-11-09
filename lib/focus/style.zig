const focus = @import("../focus.zig");
usingnamespace focus.common;

pub const background_color = Color{ .r = 0x2e, .g = 0x34, .b = 0x36, .a = 255 };
pub const status_background_color = Color{ .r = @divTrunc(0x2e, 2), .g = @divTrunc(0x34, 2), .b = @divTrunc(0x36, 2), .a = 255 };
pub const text_color = Color{ .r = 0xee, .g = 0xee, .b = 0xec, .a = 255 };
pub const highlight_color = Color{ .r = 0xee, .g = 0xee, .b = 0xec, .a = 100 };
pub const multi_cursor_color = Color{ .r = 0x7a, .g = 0xa6, .b = 0xda, .a = 255 };
