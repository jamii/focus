const focus = @import("../focus.zig");
usingnamespace focus.common;
const App = focus.App;
const Buffer = focus.Buffer;
const Editor = focus.Editor;
const Window = focus.Window;
const style = focus.style;
const SingleLineEditor = focus.SingleLineEditor;
const Selector = focus.Selector;

pub const FileOpener = struct {
    app: *App,
    preview_editor: *Editor,
    input: SingleLineEditor,
    selector: Selector,

    pub fn init(app: *App, init_path: []const u8) *FileOpener {
        const empty_buffer = Buffer.initEmpty(app, .Preview);
        const preview_editor = Editor.init(app, empty_buffer, false, false);
        const input = SingleLineEditor.init(app, init_path);
        const selector = Selector.init(app);
        const self = app.allocator.create(FileOpener) catch oom();
        self.* = FileOpener{
            .app = app,
            .preview_editor = preview_editor,
            .input = input,
            .selector = selector,
        };
        return self;
    }

    pub fn deinit(self: *FileOpener) void {
        self.selector.deinit();
        self.input.deinit();
        const buffer = self.preview_editor.buffer;
        self.preview_editor.deinit();
        buffer.deinit();
        self.app.allocator.destroy(self);
    }

    pub fn frame(self: *FileOpener, window: *Window, rect: Rect, events: []const c.SDL_Event) void {
        const layout = window.layoutSearcherWithPreview(rect);

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
        var results_err_o: ?std.fs.Dir.OpenError = null;
        {
            const path = self.input.getText();
            var dirname_o: ?[]const u8 = null;
            var basename: []const u8 = "";
            if (path.len > 0) {
                if (std.fs.path.isSep(path[path.len - 1])) {
                    dirname_o = path;
                    basename = "";
                } else {
                    dirname_o = std.fs.path.dirname(path);
                    basename = std.fs.path.basename(path);
                }
            }
            if (dirname_o) |dirname| {
                if (std.fs.cwd().openDir(dirname, .{ .iterate = true })) |*dir| {
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
                } else |err| {
                    results_err_o = err;
                }
            }
        }
        std.sort.sort([]const u8, results.items, {}, struct {
            fn lessThan(_: void, a: []const u8, b: []const u8) bool {
                return std.mem.lessThan(u8, a, b);
            }
        }.lessThan);

        // run selector frame
        var action: Selector.Action = .None;
        if (results_err_o) |results_err| {
            const error_text = format(self.app.frame_allocator, "Error opening directory: {}", .{results_err});
            window.queueText(layout.selector, style.error_text_color, error_text);
        } else {
            action = self.selector.frame(window, layout.selector, events, results.items);
        }

        const path = self.input.getText();
        const dirname = if (path.len > 0 and std.fs.path.isSep(path[path.len - 1]))
            path[0 .. path.len - 1]
        else
            std.fs.path.dirname(path) orelse "";

        // maybe open file
        if (action == .SelectRaw or action == .SelectOne) {
            const filename: []const u8 = if (action == .SelectRaw)
                std.mem.dupe(self.app.frame_allocator, u8, self.input.getText()) catch oom()
            else
                std.fs.path.join(self.app.frame_allocator, &[_][]const u8{ dirname, results.items[self.selector.selected] }) catch oom();
            if (filename.len > 0 and std.fs.path.isSep(filename[filename.len - 1])) {
                if (action == .SelectRaw)
                    std.fs.cwd().makeDir(filename) catch |err| {
                        panic("{} while creating directory {}", .{ err, filename });
                    };
                self.input.setText(filename);
            } else {
                if (action == .SelectRaw) {
                    const file = std.fs.cwd().createFile(filename, .{ .truncate = false }) catch |err| {
                        panic("{} while creating file {}", .{ err, filename });
                    };
                    file.close();
                }
                const new_buffer = self.app.getBufferFromAbsoluteFilename(filename);
                const new_editor = Editor.init(self.app, new_buffer, true, true);
                window.popView();
                window.pushView(new_editor);
            }
        }

        // update preview
        const buffer = self.preview_editor.buffer;
        self.preview_editor.deinit();
        buffer.deinit();
        if (results.items.len == 0) {
            const empty_buffer = Buffer.initEmpty(self.app, .Preview);
            self.preview_editor = Editor.init(self.app, empty_buffer, false, false);
        } else {
            const selected = results.items[self.selector.selected];
            if (std.mem.endsWith(u8, selected, "/")) {
                const empty_buffer = Buffer.initEmpty(self.app, .Preview);
                self.preview_editor = Editor.init(self.app, empty_buffer, false, false);
            } else {
                const filename = std.fs.path.join(self.app.frame_allocator, &[_][]const u8{ dirname, selected }) catch oom();
                const preview_buffer = Buffer.initFromAbsoluteFilename(self.app, .Preview, filename);
                self.preview_editor = Editor.init(self.app, preview_buffer, false, false);
            }
        }

        // run preview frame
        self.preview_editor.frame(window, layout.preview, &[0]c.SDL_Event{});
    }
};
