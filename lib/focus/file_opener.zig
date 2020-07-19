const focus = @import("../focus.zig");
usingnamespace focus.common;
const App = focus.App;
const Id = focus.Id;
const Buffer = focus.Buffer;
const Editor = focus.Editor;
const Window = focus.Window;
const style = focus.style;
const Selector = focus.Selector;

pub const FileOpener = struct {
    app: *App,
    input_buffer_id: Id,
    input_editor_id: Id,
    selector: Selector,

    pub fn init(app: *App, current_directory: []const u8) Id {
        const input_buffer_id = Buffer.initEmpty(app);
        const input_editor_id = Editor.init(app, input_buffer_id);
        var input_editor = app.getThing(input_editor_id).Editor;
        input_editor.insert(input_editor.getMainCursor(), current_directory);
        input_editor.goBufferEnd(input_editor.getMainCursor());

        const selector = Selector.init(app);

        return app.putThing(FileOpener{
            .app = app,
            .input_buffer_id = input_buffer_id,
            .input_editor_id = input_editor_id,
            .selector = selector,
        });
    }

    pub fn deinit(self: *FileOpener) void {}

    pub fn frame(self: *FileOpener, window: *Window, rect: Rect, events: []const c.SDL_Event) void {
        var input_buffer = self.app.getThing(self.input_buffer_id).Buffer;
        var input_editor = self.app.getThing(self.input_editor_id).Editor;

        // handle events
        var input_events = ArrayList(c.SDL_Event).init(self.app.frame_allocator);
        var selector_events = ArrayList(c.SDL_Event).init(self.app.frame_allocator);
        for (events) |event| {
            switch (event.type) {
                c.SDL_KEYDOWN => {
                    const sym = event.key.keysym;
                    if (sym.mod == c.KMOD_LCTRL or sym.mod == c.KMOD_RCTRL) {
                        switch (sym.sym) {
                            'q' => window.popView(),
                            'k', 'i', c.SDLK_RETURN => selector_events.append(event) catch oom(),
                            else => input_events.append(event) catch oom(),
                        }
                    } else if (sym.mod == c.KMOD_LALT or sym.mod == c.KMOD_RALT) {
                        switch (sym.sym) {
                            'k', 'i', c.SDLK_RETURN => selector_events.append(event) catch oom(),
                            else => input_events.append(event) catch oom(),
                        }
                    } else if (sym.mod == 0) {
                        switch (sym.sym) {
                            c.SDLK_RETURN => selector_events.append(event) catch oom(),
                            else => input_events.append(event) catch oom(),
                        }
                    } else {
                        input_events.append(event) catch oom();
                    }
                },
                c.SDL_MOUSEWHEEL => {},
                else => input_events.append(event) catch oom(),
            }
        }

        // remove any sneaky newlines
        {
            var pos: usize = 0;
            while (input_buffer.searchForwards(pos, "\n")) |new_pos| {
                pos = new_pos;
                input_editor.delete(pos, pos + 1);
            }
        }

        // get and filter completions
        var results = ArrayList([]const u8).init(self.app.frame_allocator);
        {
            const path = input_buffer.bytes.items;
            var dirname_o: ?[]const u8 = null;
            var basename: []const u8 = "";
            if (path.len > 0 and std.fs.path.isSep(path[path.len - 1])) {
                dirname_o = path[0 .. path.len - 1];
                basename = "";
            } else {
                dirname_o = std.fs.path.dirname(path);
                basename = std.fs.path.basename(path);
            }
            if (dirname_o) |dirname| {
                var dir = std.fs.cwd().openDir(dirname, .{ .iterate = true })
                    catch |err| panic("{} while opening dir {s}", .{err, dirname});
                defer dir.close();
                var dir_iter = dir.iterate();
                while (dir_iter.next()
                           catch |err| panic("{} while iterating dir {s}", .{err, dirname}))
                    |entry| {
                        if (std.mem.startsWith(u8, entry.name, basename)) {
                            var result = ArrayList(u8).init(self.app.frame_allocator);
                            result.appendSlice(entry.name) catch oom();
                            if (entry.kind == .Directory) result.append('/') catch oom();
                            results.append(result.toOwnedSlice()) catch oom();
                    }
                }
            }
        }

        // split rect
        var all_rect = rect;
        const target_rect = all_rect.splitTop(@divTrunc(rect.h, 2), 0);
        const border1_rect = all_rect.splitTop(@divTrunc(self.app.atlas.char_height, 8), 0);
        const input_rect = all_rect.splitBottom(self.app.atlas.char_height, 0);
        const border2_rect = all_rect.splitBottom(@divTrunc(self.app.atlas.char_height, 8), 0);
        const selector_rect = all_rect;
        window.queueRect(border1_rect, style.text_color);
        window.queueRect(border2_rect, style.text_color);

         // run selector frame
        const action = self.selector.frame(window, selector_rect, selector_events.toOwnedSlice(), results.items);

        // maybe open file
        if (action == .SelectRaw or action == .SelectOne) {
            var filename = ArrayList(u8).init(self.app.frame_allocator);
            if (action == .SelectRaw) {
                filename.appendSlice(input_buffer.bytes.items) catch oom();
            } else {
                const path = input_buffer.bytes.items;
                const dirname = if (path.len > 0 and std.fs.path.isSep(path[path.len - 1]))
                    path[0 .. path.len - 1]
                    else
                    std.fs.path.dirname(path) orelse "";
                filename.appendSlice(dirname) catch oom();
                filename.append('/') catch oom();
                filename.appendSlice(results.items[self.selector.selected]) catch oom();
            }
            if (filename.items.len > 0 and std.fs.path.isSep(filename.items[filename.items.len - 1])) {
                // TODO mkdir?
                input_buffer.bytes.shrink(0);
                input_buffer.bytes.appendSlice(filename.items) catch oom();
                input_editor.goBufferEnd(input_editor.getMainCursor());
                filename.deinit();
            } else {
                const new_buffer_id = Buffer.initFromAbsoluteFilename(self.app, filename.toOwnedSlice());
                const new_editor_id = Editor.init(self.app, new_buffer_id);
                window.popView();
                window.pushView(new_editor_id);
            }
        }

        // run other editor frames
        input_editor.frame(window, input_rect, input_events.toOwnedSlice());
    }
};
