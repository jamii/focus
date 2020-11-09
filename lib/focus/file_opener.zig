const focus = @import("../focus.zig");
usingnamespace focus.common;
const App = focus.App;
const Id = focus.Id;
const Buffer = focus.Buffer;
const Editor = focus.Editor;
const Window = focus.Window;
const style = focus.style;
const SingleLineEditor = focus.SingleLineEditor;
const Selector = focus.Selector;

pub const FileOpener = struct {
    app: *App,
    preview_buffer_id: Id,
    preview_editor_id: Id,
    input: SingleLineEditor,
    selector: Selector,

    pub fn init(app: *App, init_path: []const u8) Id {
        const preview_buffer_id = Buffer.initEmpty(app);
        const preview_editor_id = Editor.init(app, preview_buffer_id, false);
        const input = SingleLineEditor.init(app, init_path);
        const selector = Selector.init(app);
        return app.putThing(FileOpener{
            .app = app,
            .preview_buffer_id = preview_buffer_id,
            .preview_editor_id = preview_editor_id,
            .input = input,
            .selector = selector,
        });
    }

    pub fn frame(self: *FileOpener, window: *Window, rect: Rect, events: []const c.SDL_Event) void {
        const layout = window.layoutSearcher(rect);

        // handle events
        for (events) |event| {
            switch (event.type) {
                else => {},
            }
        }

        // run input frame
        self.input.frame(window, layout.input, events);

        // get and filter completions
        var results = ArrayList([]const u8).init(self.app.frame_allocator);
        {
            const path = self.input.getText();
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
                var dir = std.fs.cwd().openDir(dirname, .{ .iterate = true }) catch |err| panic("{} while opening dir {s}", .{ err, dirname });
                defer dir.close();
                var dir_iter = dir.iterate();
                while (dir_iter.next() catch |err| panic("{} while iterating dir {s}", .{ err, dirname })) |entry| {
                    if (std.mem.startsWith(u8, entry.name, basename)) {
                        var result = ArrayList(u8).init(self.app.frame_allocator);
                        result.appendSlice(entry.name) catch oom();
                        if (entry.kind == .Directory) result.append('/') catch oom();
                        results.append(result.toOwnedSlice()) catch oom();
                    }
                }
            }
        }

        // run selector frame
        const action = self.selector.frame(window, layout.selector, events, results.items);

        const path = self.input.getText();
        const dirname = if (path.len > 0 and std.fs.path.isSep(path[path.len - 1]))
            path[0 .. path.len - 1]
        else
            std.fs.path.dirname(path) orelse "";

        // maybe open file
        if (action == .SelectRaw or action == .SelectOne) {
            var filename = ArrayList(u8).init(self.app.frame_allocator);
            if (action == .SelectRaw) {
                filename.appendSlice(self.input.getText()) catch oom();
            } else {
                filename.appendSlice(dirname) catch oom();
                filename.append('/') catch oom();
                filename.appendSlice(results.items[self.selector.selected]) catch oom();
            }
            if (filename.items.len > 0 and std.fs.path.isSep(filename.items[filename.items.len - 1])) {
                // TODO mkdir?
                self.input.setText(filename.items);
            } else {
                const new_buffer_id = Buffer.initFromAbsoluteFilename(self.app, filename.toOwnedSlice());
                const new_editor_id = Editor.init(self.app, new_buffer_id, true);
                window.popView();
                window.pushView(new_editor_id);
            }
        }

        // update preview
        var preview_buffer = self.app.getThing(self.preview_buffer_id).Buffer;
        var preview_editor = self.app.getThing(self.preview_editor_id).Editor;
        if (results.items.len == 0) {
            preview_buffer.bytes.shrink(0);
        } else {
            const selected = results.items[self.selector.selected];
            if (std.mem.endsWith(u8, selected, "/")) {
                preview_buffer.bytes.shrink(0);
            } else {
                var filename = ArrayList(u8).init(self.app.frame_allocator);
                filename.appendSlice(dirname) catch oom();
                filename.append('/') catch oom();
                filename.appendSlice(selected) catch oom();
                preview_buffer.load(filename.items);
            }
        }

        // run preview frame
        preview_editor.frame(window, layout.preview, &[0]c.SDL_Event{});
    }
};
