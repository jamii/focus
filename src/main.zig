usingnamespace @import("common.zig");

const draw = @import("./draw.zig");
const Fui = @import("./fui.zig").Fui;
const Memory = @import("./memory.zig").Memory;

const root_allocator = std.heap.c_allocator;

pub fn main() anyerror!void {
    draw.init();
    var fui = Fui.init(root_allocator);
    defer fui.deinit();

    var memory = try Memory.init(root_allocator);
    defer memory.deinit();

    while (true) {
        while (!fui.handle_input()) {
            std.time.sleep(@divTrunc(std.time.second, 240));
        }
        const screen = try fui.begin();
        try memory.frame(&fui, screen);
        try fui.end();
    }
}
