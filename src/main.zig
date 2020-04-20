usingnamespace @import("common.zig");

const draw = @import("./draw.zig");
const Fui = @import("./fui.zig").Fui;
const Memory = @import("./memory.zig").Memory;

pub fn main() anyerror!void {
    draw.init();
    var fui = Fui.init();

    var arena = std.heap.ArenaAllocator.init(std.heap.c_allocator);
    defer arena.deinit();

    var memory = try Memory.init(&arena);
    dump(memory.queue);

    // main loop
    while (true) {
        if (fui.handle_input()) {
            try memory.frame(&fui);
        }
        draw.clear(.{.r=0, .g=0, .b=0, .a=255});
        draw.set_clip(.{.x=0, .y=0, .w=100, .h=100});
        draw.swap();
        std.time.sleep(@divTrunc(std.time.second, 120));
    }
}
