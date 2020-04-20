usingnamespace @import("common.zig");

const c_allocator = std.heap.c_allocator;

const bg: Color = Color{.r=0, .g=0, .b=0, .a=255};

pub fn main() anyerror!void {
    draw.init();

    var arena = std.heap.ArenaAllocator.init(c_allocator);
    defer arena.deinit();

    const the_memory = try memory.Memory.init(&arena);
    d(the_memory);

    // main loop
    while (true) {

        // handle SDL events
        var e: c.SDL_Event = undefined;
        var got_input = false;
        while (c.SDL_PollEvent(&e) != 0) {
            got_input = true;
            switch (e.type) {
                c.SDL_QUIT => std.os.exit(0),
                // SDL_MOUSEMOTION => mu_input_mousemove(ctx, e.motion.x, e.motion.y),
                // SDL_MOUSEWHEEL => mu_input_scroll(ctx, 0, e.wheel.y * -30),
                // SDL_TEXTINPUT => mu_input_text(ctx, &e.text.text),
                // SDL_MOUSEBUTTONDOWN, SDL_MOUSEBUTTONUP => {
                //     const ob: ?c_int = switch(e.button.button & 0xff) {
                //         SDL_BUTTON_LEFT => MU_MOUSE_LEFT,
                //         SDL_BUTTON_RIGHT => MU_MOUSE_RIGHT,
                //         SDL_BUTTON_MIDDLE => MU_MOUSE_MIDDLE,
                //         else => null
                //     };
                //     if (ob) |b| {
                //         switch (e.type) {
                //             SDL_MOUSEBUTTONDOWN => mu_input_mousedown(ctx, e.button.x, e.button.y, b),
                //             SDL_MOUSEBUTTONUP => mu_input_mouseup(ctx, e.button.x, e.button.y, b),
                //             else => unreachable,
                //         }
                //     }
                // },
                // SDL_KEYDOWN, SDL_KEYUP => {
                //     const oc: ?c_int = switch (e.key.keysym.sym & 0xff) {
                //         SDLK_LSHIFT => MU_KEY_SHIFT,
                //         SDLK_RSHIFT => MU_KEY_SHIFT,
                //         SDLK_LCTRL => MU_KEY_CTRL,
                //         SDLK_RCTRL => MU_KEY_CTRL,
                //         SDLK_LALT => MU_KEY_ALT,
                //         SDLK_RALT => MU_KEY_ALT,
                //         SDLK_RETURN => MU_KEY_RETURN,
                //         SDLK_BACKSPACE => MU_KEY_BACKSPACE,
                //         else => null
                //     };
                //     if (oc) |c| {
                //         switch (e.type) {
                //             SDL_KEYDOWN => mu_input_keydown(ctx, c),
                //             SDL_KEYUP => mu_input_keyup(ctx, c),
                //             else => unreachable,
                //         }
                //     }
                // },
                else => {}
            }
        }

        if (got_input) {
            // render
            draw.clear(bg);
            draw.set_clip(.{.x=0, .y=0, .w=100, .h=100});
            draw.rect(.{.x=0, .y=0, .w=100, .h=100}, .{.r=100, .g=255, .b=0, .a=255});
            draw.text("hello!", .{.x=0, .y=0}, .{.r=255, .g=255, .b=0, .a=255});
            draw.swap();
        }

        std.time.sleep(@divTrunc(1_000_000_000, 120));
    }
}
