const focus = @import("../focus.zig");
usingnamespace focus.common;
const Buffer = focus.Buffer;
const Editor = focus.Editor;
const Window = focus.Window;
const App = focus.App;

pub const FileOpener = struct {
    allocator: *Allocator,
    buffer: *Buffer,
    editor: Editor,

    pub fn init(allocator: *Allocator, current_directory: []const u8) ! FileOpener {
        var buffer = try allocator.create(Buffer);
        buffer.* = Buffer.init(allocator);
        try buffer.insert(0, current_directory);
        var editor = try Editor.init(allocator, buffer);
        editor.goBufferEnd(editor.getMainCursor());
        return FileOpener{
            .allocator = allocator,
            .buffer = buffer,
            .editor = editor,
        };
    }

    pub fn deinit(self: *FileOpener) void {
        self.editor.deinit();
        self.buffer.deinit();
        try self.allocator.destroy(self.buffer);
    }

    pub fn frame(self: *FileOpener, app: *App, window: *Window, rect: Rect, events: []const c.SDL_Event) ! void {
        var editor_events = ArrayList(c.SDL_Event).init(self.allocator);
        defer editor_events.deinit();

        // handle events
        for (events) |event| {
            var handled = false;
            switch (event.type) {
                c.SDL_KEYDOWN => {
                    const sym = event.key.keysym;
                    if (sym.mod == c.KMOD_LCTRL or sym.mod == c.KMOD_RCTRL) {
                        switch (sym.sym) {
                            'q' => {
                                window.popView();
                                handled = true;
                            },
                            else => {},
                        }
                    }
                    if (sym.mod == 0) {
                        switch (sym.sym) {
                            c.SDLK_RETURN => {
                                // TODO actually open file :)
                                window.popView();
                                try window.pushView(.{.Editor = self.editor});
                                handled = true;
                            },
                            else => {},
                        }
                    }
                },
                c.SDL_MOUSEWHEEL => handled = true,
                else => {},
            }
            // delegate other events to editor
            if (!handled) try editor_events.append(event);
        }

        // run editor frame
        const editor_rect = Rect{
            .x = rect.x,
            .y = rect.y,
            .w = rect.w,
            .h = app.atlas.char_height,
        };
        try self.editor.frame(app, window, editor_rect, editor_events.items);

        // remove any sneaky newlines
        {
            var pos: usize = 0;
            while (self.buffer.searchForwards(pos, "\n")) |new_pos| {
                pos = new_pos;
                self.buffer.delete(pos, pos+1);
            }
        }

        // TODO render autocomplete
    }
};
