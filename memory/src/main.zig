usingnamespace @import("common.zig");
usingnamespace @import("renderer.zig");

pub const screen_height = 1440;
pub const screen_width = 720;

const allocator = std.testing.allocator;

const bg: [3]f32 = [3]f32{ 50, 50, 50 };

fn text_width(font: mu_Font, text: [*c]const u8, len: c_int) callconv(.C) c_int {
    return r_get_text_width(text, if (len == -1) @intCast(c_int, strlen(text)) else len);
}

fn text_height(font: mu_Font) callconv(.C) c_int {
    return r_get_text_height();
}

pub fn main() anyerror!void {
    // init SDL and renderer
    _ = SDL_Init(SDL_INIT_EVERYTHING);
    r_init();

    // init microui
    var ctx: *mu_Context = try allocator.create(mu_Context);
    defer allocator.destroy(&ctx);
    mu_init(ctx);
    ctx.text_width = text_width;
    ctx.text_height = text_height;

    // main loop
    while (true) {

        // handle SDL events
        var e: SDL_Event = undefined;
        while (SDL_PollEvent(&e) != 0) {
            switch (e.type) {
                SDL_QUIT => exit(EXIT_SUCCESS),
                SDL_MOUSEMOTION => mu_input_mousemove(ctx, e.motion.x, e.motion.y),
                SDL_MOUSEWHEEL => mu_input_scroll(ctx, 0, e.wheel.y * -30),
                SDL_TEXTINPUT => mu_input_text(ctx, &e.text.text),
                SDL_MOUSEBUTTONDOWN, SDL_MOUSEBUTTONUP => {
                    const ob: ?c_int = switch(e.button.button & 0xff) {
                        SDL_BUTTON_LEFT => MU_MOUSE_LEFT,
                        SDL_BUTTON_RIGHT => MU_MOUSE_RIGHT,
                        SDL_BUTTON_MIDDLE => MU_MOUSE_MIDDLE,
                        else => null
                    };
                    if (ob) |b| {
                        switch (e.type) {
                            SDL_MOUSEBUTTONDOWN => mu_input_mousedown(ctx, e.button.x, e.button.y, b),
                            SDL_MOUSEBUTTONUP => mu_input_mouseup(ctx, e.button.x, e.button.y, b),
                            else => unreachable,
                        }
                    }
                },
                SDL_KEYDOWN, SDL_KEYUP => {
                    const oc: ?c_int = switch (e.key.keysym.sym & 0xff) {
                        SDLK_LSHIFT => MU_KEY_SHIFT,
                        SDLK_RSHIFT => MU_KEY_SHIFT,
                        SDLK_LCTRL => MU_KEY_CTRL,
                        SDLK_RCTRL => MU_KEY_CTRL,
                        SDLK_LALT => MU_KEY_ALT,
                        SDLK_RALT => MU_KEY_ALT,
                        SDLK_RETURN => MU_KEY_RETURN,
                        SDLK_BACKSPACE => MU_KEY_BACKSPACE,
                        else => null
                    };
                    if (oc) |c| {
                        switch (e.type) {
                            SDL_KEYDOWN => mu_input_keydown(ctx, c),
                            SDL_KEYUP => mu_input_keyup(ctx, c),
                            else => unreachable,
                        }
                    }
                },
                else => {}
            }
        }

        // process frame
        mu_begin(ctx);
        if (mu_begin_window_ex(ctx, "Log Window", mu_rect(350, 40, 300, 200), 0) != 0) {
            const win: *mu_Container = mu_get_current_container(ctx);
            win.rect.w = std.math.max(win.rect.w, 240);
            win.rect.h = std.math.max(win.rect.h, 300);

            mu_layout_row(ctx, 2, &[_]c_int{ -70, -1 }, 0);
            _ = mu_button_ex(ctx, "Submit", 0, MU_OPT_ALIGNCENTER);
            mu_end_window(ctx);
        }
        mu_end(ctx);

        // render
        r_clear(mu_color(bg[0], bg[1], bg[2], 255));
        var cmd: [*c]mu_Command = null;
        while (mu_next_command(ctx, &cmd) != 0) {
            switch (cmd.*.type) {
                MU_COMMAND_TEXT => r_draw_text(&cmd.*.text.str, cmd.*.text.pos, cmd.*.text.color),
                MU_COMMAND_RECT => r_draw_rect(cmd.*.rect.rect, cmd.*.rect.color),
                MU_COMMAND_ICON => r_draw_icon(cmd.*.icon.id, cmd.*.icon.rect, cmd.*.icon.color),
                MU_COMMAND_CLIP => r_set_clip_rect(cmd.*.clip.rect),
                else => {},
            }
        }
        r_present();
    }
}
