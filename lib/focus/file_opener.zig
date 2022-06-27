const std = @import("std");
const focus = @import("../focus.zig");
const u = focus.util;
const c = focus.util.c;
const App = focus.App;
const Buffer = focus.Buffer;
const Editor = focus.Editor;
const Window = focus.Window;
const style = focus.style;
const SingleLineEditor = focus.SingleLineEditor;
const Selector = focus.Selector;
const mach_compat = focus.mach_compat;

pub const FileOpener = struct {
    app: *App,
    preview_editor: *Editor,
    input: SingleLineEditor,
    selector: Selector,

    pub fn init(app: *App, init_path: []const u8) *FileOpener {
        const empty_buffer = Buffer.initEmpty(app, .{
            .limit_load_bytes = true,
            .enable_completions = false,
            .enable_undo = false,
        });
        const preview_editor = Editor.init(app, empty_buffer, .{
            .show_status_bar = false,
            .show_completer = false,
        });
        const input = SingleLineEditor.init(app, init_path);
        const selector = Selector.init(app);
        const self = app.allocator.create(FileOpener) catch u.oom();
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

    pub fn frame(self: *FileOpener, window: *Window, rect: u.Rect, events: []const mach_compat.Event) void {
        const layout = window.layoutSearcherWithPreview(rect);

        // no event handling

        // run input frame
        const input_changed = self.input.frame(window, layout.input, events);
        if (input_changed == .Changed) self.selector.selected = 0;

        // get and filter completions
        const results_or_err = u.fuzzy_search_paths(self.app.frame_allocator, self.input.getText());
        const results = results_or_err catch &[_][]const u8{};

        // run selector frame
        var action: Selector.Action = .None;
        if (results_or_err) |_| {
            self.selector.setItems(results);
            action = self.selector.frame(window, layout.selector, events);
        } else |results_err| {
            const error_text = u.format(self.app.frame_allocator, "Error opening directory: {}", .{results_err});
            window.queueText(layout.selector, style.emphasisRed, error_text);
        }

        const path = self.input.getText();
        const dirname = self.app.frame_allocator.dupe(
            u8,
            if (path.len > 0 and std.fs.path.isSep(path[path.len - 1]))
                path[0 .. path.len - 1]
            else
                std.fs.path.dirname(path) orelse "",
        ) catch u.oom();

        // maybe open file
        if (action == .SelectRaw or action == .SelectOne) {
            const filename: []const u8 = if (action == .SelectRaw)
                self.app.frame_allocator.dupe(u8, self.input.getText()) catch u.oom()
            else
                std.fs.path.join(self.app.frame_allocator, &[_][]const u8{ dirname, results[self.selector.selected] }) catch u.oom();
            if (filename.len > 0 and std.fs.path.isSep(filename[filename.len - 1])) {
                if (action == .SelectRaw)
                    std.fs.cwd().makeDir(filename) catch |err| {
                        u.panic("{} while creating directory {s}", .{ err, filename });
                    };
                self.input.setText(filename);
                self.selector.selected = 0;
            } else {
                if (action == .SelectRaw) {
                    const file = std.fs.cwd().createFile(filename, .{ .truncate = false }) catch |err| {
                        u.panic("{} while creating file {s}", .{ err, filename });
                    };
                    file.close();
                }
                const new_buffer = self.app.getBufferFromAbsoluteFilename(filename);
                const new_editor = Editor.init(self.app, new_buffer, .{});
                window.popView();
                window.pushView(new_editor);
            }
        }

        // update preview
        const buffer = self.preview_editor.buffer;
        self.preview_editor.deinit();
        buffer.deinit();
        if (self.selector.selected >= results.len) {
            const empty_buffer = Buffer.initEmpty(self.app, .{
                .limit_load_bytes = true,
                .enable_completions = false,
                .enable_undo = false,
            });
            self.preview_editor = Editor.init(self.app, empty_buffer, .{
                .show_status_bar = false,
                .show_completer = false,
            });
        } else {
            const selected = results[self.selector.selected];
            if (std.mem.endsWith(u8, selected, "/")) {
                const empty_buffer = Buffer.initEmpty(self.app, .{
                    .limit_load_bytes = true,
                    .enable_completions = false,
                    .enable_undo = false,
                });
                self.preview_editor = Editor.init(self.app, empty_buffer, .{
                    .show_status_bar = false,
                    .show_completer = false,
                });
            } else {
                const filename = std.fs.path.join(self.app.frame_allocator, &[_][]const u8{ dirname, selected }) catch u.oom();

                const preview_buffer = Buffer.initFromAbsoluteFilename(self.app, .{
                    .limit_load_bytes = true,
                    .enable_completions = false,
                    .enable_undo = false,
                }, filename);
                self.preview_editor = Editor.init(self.app, preview_buffer, .{
                    .show_status_bar = false,
                    .show_completer = false,
                });
            }
        }

        // run preview frame
        self.preview_editor.frame(window, layout.preview, &[0]mach_compat.Event{});
    }
};
