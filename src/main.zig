usingnamespace @import("common.zig");

const draw = @import("./draw.zig");
const UI = @import("./ui.zig").UI;
const Memory = @import("./memory.zig").Memory;

const root_allocator = std.heap.c_allocator;

pub fn main() anyerror!void {
    draw.init();
    var ui = UI.init(root_allocator);
    defer ui.deinit();

    var memory = try Memory.init(root_allocator);
    defer memory.deinit();

    while (true) {
        _ = ui.handleInput();
        const screen = try ui.begin();
        try memory.frame(&ui, screen);
        try ui.end();
    }
}
