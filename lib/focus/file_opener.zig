const focus = @import("../focus.zig");
usingnamespace focus.common;
const App = focus.App;
const Id = focus.Id;
const Buffer = focus.Buffer;
const Editor = focus.Editor;
const Window = focus.Window;
const style = focus.style;

pub const FileOpener = struct {
    app: *App,
    input_buffer_id: Id,
    input_editor_id: Id,
    completions_buffer_id: Id,
    completions_editor_id: Id,
    selected: usize, // 0 for nothing selected, i-1 for line i

    pub fn init(app: *App, current_directory: []const u8) Id {
        // TODO don't directly mutate buffer - messes up multiple cursors - go via editor instead
        const input_buffer_id = Buffer.initEmpty(app);
        const input_editor_id = Editor.init(app, input_buffer_id);
        const completions_buffer_id = Buffer.initEmpty(app);
        const completions_editor_id = Editor.init(app, completions_buffer_id);

        // default to current directory
        app.getThing(input_buffer_id).Buffer.insert(0, current_directory);

        // start cursor at end
        var input_editor = app.getThing(input_editor_id).Editor;
        input_editor.goBufferEnd(input_editor.getMainCursor());

        return app.putThing(FileOpener{
            .app = app,
            .input_buffer_id = input_buffer_id,
            .input_editor_id = input_editor_id,
            .completions_buffer_id = completions_buffer_id,
            .completions_editor_id = completions_editor_id,
            .selected = 0,
        });
    }

    pub fn deinit(self: *FileOpener) void {}

    pub fn frame(self: *FileOpener, window: *Window, rect: Rect, events: []const c.SDL_Event) void {
        var input_buffer = self.app.getThing(self.input_buffer_id).Buffer;
        var input_editor = self.app.getThing(self.input_editor_id).Editor;
        var completions_buffer = self.app.getThing(self.completions_buffer_id).Buffer;
        var completions_editor = self.app.getThing(self.completions_editor_id).Editor;

        // handle events
        var input_editor_events = ArrayList(c.SDL_Event).init(self.app.frame_allocator);
        for (events) |event| {
            var delegate = false;
            switch (event.type) {
                c.SDL_KEYDOWN => {
                    const sym = event.key.keysym;
                    if (sym.mod == c.KMOD_LCTRL or sym.mod == c.KMOD_RCTRL) {
                        switch (sym.sym) {
                            'q' => window.popView(),
                            'k' => self.selected += 1,
                            'i' => if (self.selected != 0) {
                                self.selected -= 1;
                            },
                            else => delegate = true,
                        }
                    } else if (sym.mod == c.KMOD_LALT or sym.mod == c.KMOD_RALT) {
                        switch (sym.sym) {
                            'k' => self.selected = completions_buffer.countLines() - 1,
                            'i' => self.selected = 1,
                            else => delegate = true,
                        }
                    } else if (sym.mod == 0) {
                        switch (sym.sym) {
                            c.SDLK_RETURN => {
                                var filename = ArrayList(u8).init(self.app.frame_allocator);
                                if (self.selected == 0) {
                                    filename.appendSlice(input_buffer.bytes.items) catch oom();
                                } else {
                                    const path = input_buffer.bytes.items;
                                    const dirname = if (path.len > 0 and std.fs.path.isSep(path[path.len - 1]))
                                        path[0 .. path.len - 1]
                                    else
                                        std.fs.path.dirname(path) orelse "";
                                    filename.appendSlice(dirname) catch oom();
                                    filename.append('/') catch oom();
                                    const selection = completions_editor.dupeSelection(self.app.frame_allocator, completions_editor.getMainCursor());
                                    filename.appendSlice(selection) catch oom();
                                }
                                if (filename.items.len > 0 and std.fs.path.isSep(filename.items[filename.items.len - 1])) {
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
                            },
                            c.SDLK_TAB => {
                                var min_common_prefix_o: ?[]const u8 = null;
                                var lines_iter = std.mem.split(completions_buffer.bytes.items, "\n");
                                while (lines_iter.next()) |line| {
                                    if (line.len != 0) {
                                        if (min_common_prefix_o) |min_common_prefix| {
                                            var i: usize = 0;
                                            while (i < min(min_common_prefix.len, line.len) and min_common_prefix[i] == line[i]) i += 1;
                                            min_common_prefix_o = line[0..i];
                                        } else {
                                            min_common_prefix_o = line;
                                        }
                                    }
                                }
                                if (min_common_prefix_o) |min_common_prefix| {
                                    const path = input_buffer.bytes.items;
                                    const dirname = if (path.len > 0 and std.fs.path.isSep(path[path.len - 1]))
                                        path[0 .. path.len - 1]
                                    else
                                        std.fs.path.dirname(path) orelse "";
                                    // TODO is dirname always a prefix of path?
                                    input_buffer.delete(dirname.len, input_buffer.getBufferEnd());
                                    input_buffer.insert(input_buffer.getBufferEnd(), "/");
                                    input_buffer.insert(input_buffer.getBufferEnd(), min_common_prefix);
                                    input_editor.goPos(input_editor.getMainCursor(), input_buffer.getBufferEnd());
                                }
                                self.selected = 0;
                            },
                            else => delegate = true,
                        }
                    } else {
                        delegate = true;
                    }
                },
                c.SDL_MOUSEWHEEL => {},
                else => delegate = true,
            }
            // delegate other events to input editor
            if (delegate) input_editor_events.append(event) catch oom();
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
        {
            completions_buffer.bytes.shrink(0);
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
                        completions_buffer.bytes.appendSlice(entry.name) catch oom();
                        if (entry.kind == .Directory) completions_buffer.bytes.append('/') catch oom();
                        completions_buffer.bytes.append('\n') catch oom();
                    }
                }
            }
        }

        // set selection
        self.selected = min(self.selected, completions_buffer.countLines());
        var cursor = completions_editor.getMainCursor();
        if (self.selected != 0) {
            completions_editor.goPos(cursor, completions_buffer.getPosForLineCol(self.selected - 1, 0));
            completions_editor.setMark();
            completions_editor.goLineEnd(cursor);
        } else {
            completions_editor.clearMark();
            completions_editor.goBufferStart(cursor);
        }

        // run editor frames
        var completions_rect = rect;
        const input_rect = completions_rect.splitTop(self.app.atlas.char_height, 0);
        const border_rect = completions_rect.splitTop(@divTrunc(self.app.atlas.char_height, 8), 0);
        input_editor.frame(window, input_rect, input_editor_events.items);
        window.queueRect(border_rect, style.text_color);
        completions_editor.frame(window, completions_rect, &[0]c.SDL_Event{});
    }
};
