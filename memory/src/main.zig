usingnamespace @import("common.zig");
const Plumbing = @import("plumbing.zig").Plumbing;

// enum {EASY, HARD};
var op: u8 = 0;
var property: c_int = 20;

pub fn main() anyerror!void {
    var plumbing = Plumbing.init();
    defer plumbing.deinit();

    var is_running = true;
    while (is_running) {
        plumbing.handle_input(&is_running);

        var ctx = &plumbing.nk.ctx;
        if (nk_true == nk_begin(ctx, "Demo", nk_rect(0, 0, window_width, window_height), 0))
            {
                nk_menubar_begin(ctx);
                nk_layout_row_begin(ctx, .NK_STATIC, 25, 2);
                nk_layout_row_push(ctx, 45);
                if (nk_true == nk_menu_begin_label(ctx, "FILE", NK_TEXT_LEFT, nk_vec2(120, 200))) {
                    nk_layout_row_dynamic(ctx, 30, 1);
                    _ = nk_menu_item_label(ctx, "OPEN", NK_TEXT_LEFT);
                    _ = nk_menu_item_label(ctx, "CLOSE", NK_TEXT_LEFT);
                    nk_menu_end(ctx);
                }
                nk_layout_row_push(ctx, 45);
                if (nk_true == nk_menu_begin_label(ctx, "EDIT", NK_TEXT_LEFT, nk_vec2(120, 200))) {
                    nk_layout_row_dynamic(ctx, 30, 1);
                    _ = nk_menu_item_label(ctx, "COPY", NK_TEXT_LEFT);
                    _ = nk_menu_item_label(ctx, "CUT", NK_TEXT_LEFT);
                    _ = nk_menu_item_label(ctx, "PASTE", NK_TEXT_LEFT);
                    nk_menu_end(ctx);
                }
                nk_layout_row_end(ctx);
                nk_menubar_end(ctx);

                nk_layout_row_static(ctx, 30, 80, 1);
                if (nk_true == nk_button_label(ctx, "button"))
                    _ = fprintf(stdout, "button pressed\n");
                nk_layout_row_dynamic(ctx, 30, 2);
                if (nk_true == nk_option_label(ctx, "easy", nk_bool(op == 0))) op = 0;
                if (nk_true == nk_option_label(ctx, "hard", nk_bool(op == 1))) op = 1;
                nk_layout_row_dynamic(ctx, 25, 1);
                nk_property_int(ctx, "Compression:", 0, &property, 100, 10, 1);
        }
        nk_end(ctx);

        plumbing.draw();
    }

    warn("fin\n", .{});
}

fn nk_bool(b: bool) c_int {
    return if (b) nk_true else nk_false;
}
