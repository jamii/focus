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
        const buffer_id = try Buffer.initEmpty(app);
        const editor_id = try Editor.init(app, buffer_id);
        var self = FileOpener{
            .app = app,
            .buffer_id = buffer_id,
            .editor_id = editor_id,
        };
        try self.getBuffer().insert(0, current_directory);
        self.getEditor().goBufferEnd(self.getEditor().getMainCursor());
        return app.putThing(self);
    }

    pub fn deinit(self: *FileOpener) void {
    }

    pub fn getBuffer(self: *FileOpener) *Buffer {
        return self.app.getThing(self.buffer_id).Buffer;
    }

    pub fn getEditor(self: *FileOpener) *Editor {
        return self.app.getThing(self.editor_id).Editor;
    }

    pub fn frame(self: *FileOpener, window: *Window, rect: Rect, events: []const c.SDL_Event) ! void {
        var completions_rect = rect;
        const editor_rect = completions_rect.splitTop(self.app.atlas.char_height, 0);
        
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
                                const filename = try self.getBuffer().dupe(self.app.allocator, 0, self.getBuffer().getBufferEnd());
                                errdefer self.app.allocator.free(filename);
                                const new_buffer_id = try Buffer.initFromFilename(self.app, filename);
                                const new_editor_id = try Editor.init(self.app, new_buffer_id);
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
        try self.getEditor().frame(window, editor_rect, editor_events.items);

        // remove any sneaky newlines
        {
            var pos: usize = 0;
            while (self.getBuffer().searchForwards(pos, "\n")) |new_pos| {
                pos = new_pos;
                self.getBuffer().delete(pos, pos+1);
            }
        }

        // get completions
        var completions = ArrayList([]const u8).init(self.app.allocator);
        // TODO need to figure out lifetimes for rendered text - probably do atlas lookup at queue time
        // defer {
        //     for (completions.items) |completion| {
        //         self.app.allocator.free(completion);
        //     }
        //     completions.deinit();
        // }
        {
            const path = self.getBuffer().bytes.items;
            var dirname_o: ?[]const u8 = null;
            var basename: []const u8 = "";
            if (path.len > 0 and std.fs.path.isSep(path[path.len-1])) {
                dirname_o = path;
                basename = "";
            } else {
                dirname_o = std.fs.path.dirname(path);
                basename = std.fs.path.basename(path);
            }
            if (dirname_o) |dirname| {
                var dir = try std.fs.cwd().openDir(dirname, .{.iterate=true});
                defer dir.close();
                var dir_iter = dir.iterate();
                while (try dir_iter.next()) |entry| {
                    if (std.mem.startsWith(u8, entry.name, basename)) {
                        try completions.append(try std.mem.dupe(self.app.allocator, u8, entry.name));
                    }
                }
            }
        }

        // render autocomplete
        const text_color = Color{ .r = 0xee, .g = 0xee, .b = 0xec, .a = 255 };
        for (completions.items) |completion, i| {
            try window.queueText(
                .{
                    .x = completions_rect.x,
                    .y = completions_rect.y + (@intCast(Coord, i) * self.app.atlas.char_height),
                },
                text_color,
                completion,
            );
        }
    }
};
