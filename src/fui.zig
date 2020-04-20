usingnamespace @import("common.zig");

pub const Fui = struct {
    key: ?u8,

    pub fn init() Fui {
        return Fui{
            .key = null,
        };
    }

    fn handle_input(self: *Fui) bool {
        var got_input = false;
        var e: c.SDL_Event = undefined;
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
                c.SDL_KEYDOWN, c.SDL_KEYUP => {
                    switch (e.type) {
                        c.SDL_KEYDOWN => self.key = @intCast(u8, e.key.keysym.sym & 0xff),
                        c.SDL_KEYUP => self.key = null,
                        else => unreachable,
                    }
                },
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
        return got_input;
    }
};
