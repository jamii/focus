const focus = @import("../focus.zig");
usingnamespace focus.common;
const App = focus.App;
const Id = focus.Id;
const Buffer = focus.Buffer;
const Editor = focus.Editor;
const Window = focus.Window;

pub const FileOpener = struct {
    app: *App,
    buffer_id: Id,
    editor_id: Id,

    pub fn init(app: *App, current_directory: []const u8) ! Id {
        const buffer_id = try Buffer.init(app);
        const editor_id = try Editor.init(app, buffer_id);
        var self = FileOpener{
            .app = app,
            .buffer_id = buffer_id,
            .editor_id = editor_id,
        };
        try self.buffer().insert(0, current_directory);
        self.editor().goBufferEnd(self.editor().getMainCursor());
        return app.putThing(self);
    }

    pub fn deinit(self: *FileOpener) void {
    }

    pub fn buffer(self: *FileOpener) *Buffer {
        return self.app.getThing(self.buffer_id).Buffer;
    }

    pub fn editor(self: *FileOpener) *Editor {
        return self.app.getThing(self.editor_id).Editor;
    }

    pub fn frame(self: *FileOpener, window: *Window, rect: Rect, events: []const c.SDL_Event) ! void {
        var editor_events = ArrayList(c.SDL_Event).init(self.app.allocator);
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
                                const new_buffer_id = try Buffer.init(self.app);
                                const new_editor_id = try Editor.init(self.app, new_buffer_id);
                                var new_buffer = self.app.getThing(new_buffer_id).Buffer;
                                
                                const filename = try self.buffer().dupe(self.app.allocator, 0, self.buffer().getBufferEnd());
                                defer self.app.allocator.free(filename);
                                const file = try std.fs.cwd().createFile(filename, .{.read=true, .truncate=false});
                                const chunk_size = 1024;
                                var buf = try self.app.allocator.alloc(u8, chunk_size);
                                defer self.app.allocator.free(buf);
                                while (true) {
                                    const len = try file.readAll(buf);
                                    try new_buffer.bytes.appendSlice(buf[0..len]);
                                    if (len < chunk_size) break;
                                }
                                    
                                window.popView();
                                try window.pushView(new_editor_id);
                                
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
            .h = self.app.atlas.char_height,
        };
        try self.editor().frame(window, editor_rect, editor_events.items);

        // remove any sneaky newlines
        {
            var pos: usize = 0;
            while (self.buffer().searchForwards(pos, "\n")) |new_pos| {
                pos = new_pos;
                self.buffer().delete(pos, pos+1);
            }
        }

        // TODO render autocomplete
    }
};
