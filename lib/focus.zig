pub const common = @import("./focus/common.zig");
pub const meta = @import("./focus/meta.zig");
pub const Atlas = @import("./focus/atlas.zig").Atlas;
pub const draw = @import("./focus/draw.zig");
pub const UI = @import("./focus/ui.zig").UI;
pub const Memory = @import("./focus/memory.zig").Memory;
pub const editor = @import("./focus/editor.zig");

usingnamespace common;

pub fn run(allocator: *Allocator) !void {
    var atlas = try Atlas.init(allocator);
    defer atlas.deinit();
    draw.init(&atlas);
    var ui = UI.init(allocator, &atlas);
    defer ui.deinit();

    // var memory = try Memory.init(allocator);
    // defer memory.deinit();

    var buffer = editor.Buffer.init(allocator);
    try buffer.insert(0, "some initial text\nand some more\nshort\nreaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaally long");
    var view = try editor.View.init(allocator, &buffer);

    while (true) {
        // var timer = try std.time.Timer.start();
        _ = try ui.handleInput();
        const screen = try ui.begin();
        // try memory.frame(&ui, screen);
        try view.frame(&ui, screen);
        try ui.end();
        // warn("frame time: {}ns\n", .{timer.read()});
    }
}
