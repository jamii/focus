const std = @import("std");
const focus = @import("../focus.zig");
const u = focus.util;
const c = focus.util.c;
const App = focus.App;
const Buffer = focus.Buffer;
const Editor = focus.Editor;
const SingleLineEditor = focus.SingleLineEditor;
const Window = focus.Window;
const style = focus.style;
const Selector = focus.Selector;

const projects = [_][]const u8{
    "/home/jamie/exo/",
    "/home/jamie/exo-secret/",
    "/home/jamie/imp/",
    "/home/jamie/focus/",
    "/home/jamie/tower/",
    "/home/jamie/zig/",
    "/home/jamie/blog",
    "/home/jamie/dida",
    "/home/jamie/mutant",
    "/home/jamie/inspector-z",
};

pub const ProjectFileOpener = struct {
    app: *App,
    preview_editor: *Editor,
    input: SingleLineEditor,
    selector: Selector,
    paths: []const []const u8,

    pub fn init(app: *App) *ProjectFileOpener {
        const empty_buffer = Buffer.initEmpty(app, .{
            .limit_load_bytes = true,
            .enable_completions = false,
            .enable_undo = false,
        });
        const preview_editor = Editor.init(app, empty_buffer, .{
            .show_status_bar = false,
            .show_completer = false,
        });
        const input = SingleLineEditor.init(app, app.last_file_filter);
        input.editor.goRealLineStart(input.editor.getMainCursor());
        input.editor.setMark();
        input.editor.goRealLineEnd(input.editor.getMainCursor());
        var selector = Selector.init(app);
        selector.selected = app.last_project_file_opener_selected;

        var paths = u.ArrayList([]const u8).init(app.allocator);
        for (projects) |project| {
            const result = std.ChildProcess.exec(.{
                .allocator = app.frame_allocator,
                .argv = &[_][]const u8{ "rg", "--files", "-0" },
                .cwd = project,
                .max_output_bytes = 128 * 1024 * 1024,
            }) catch |err| u.panic("{} while calling rg", .{err});
            u.assert(result.term == .Exited and result.term.Exited == 0);
            var lines = std.mem.split(u8, result.stdout, &[1]u8{0});
            while (lines.next()) |line| {
                const path = std.fs.path.join(app.allocator, &[2][]const u8{ project, line }) catch u.oom();
                paths.append(path) catch u.oom();
            }
        }

        std.sort.sort([]const u8, paths.items, {}, struct {
            fn lessThan(_: void, a: []const u8, b: []const u8) bool {
                return std.mem.lessThan(u8, a, b);
            }
        }.lessThan);

        const self = app.allocator.create(ProjectFileOpener) catch u.oom();
        self.* = ProjectFileOpener{
            .app = app,
            .preview_editor = preview_editor,
            .input = input,
            .selector = selector,
            .paths = paths.toOwnedSlice(),
        };
        return self;
    }

    pub fn deinit(self: *ProjectFileOpener) void {
        for (self.paths) |completion| {
            self.app.allocator.free(completion);
        }
        self.app.allocator.free(self.paths);
        self.selector.deinit();
        self.input.deinit();
        const buffer = self.preview_editor.buffer;
        self.preview_editor.deinit();
        buffer.deinit();
        self.app.allocator.destroy(self);
    }

    pub fn frame(self: *ProjectFileOpener, window: *Window, rect: u.Rect, events: []const c.SDL_Event) void {
        const layout = window.layoutSearcherWithPreview(rect);

        // run input frame
        const input_changed = self.input.frame(window, layout.input, events);
        if (input_changed == .Changed) self.selector.selected = 0;

        // filter paths
        const filtered_paths = u.fuzzy_search(self.app.frame_allocator, self.paths, self.input.getText());

        // run selector frame
        self.selector.setItems(filtered_paths);
        const action = self.selector.frame(window, layout.selector, events);

        // maybe open file
        if (action == .SelectOne and filtered_paths.len > 0) {
            const path = filtered_paths[self.selector.selected];
            if (path.len > 0 and std.fs.path.isSep(path[path.len - 1])) {
                self.input.setText(path);
            } else {
                const new_buffer = self.app.getBufferFromAbsoluteFilename(path);
                const new_editor = Editor.init(self.app, new_buffer, .{});
                window.popView();
                window.pushView(new_editor);
            }
        }

        // set cached search text
        self.app.allocator.free(self.app.last_file_filter);
        self.app.last_file_filter = self.app.dupe(self.input.getText());
        self.app.last_project_file_opener_selected = self.selector.selected;

        // update preview
        const buffer = self.preview_editor.buffer;
        self.preview_editor.deinit();
        buffer.deinit();
        if (filtered_paths.len == 0) {
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
            const selected = filtered_paths[self.selector.selected];
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
                const preview_buffer = Buffer.initFromAbsoluteFilename(self.app, .{
                    .limit_load_bytes = true,
                    .enable_completions = false,
                    .enable_undo = false,
                }, selected);
                self.preview_editor = Editor.init(self.app, preview_buffer, .{
                    .show_status_bar = false,
                    .show_completer = false,
                });
            }
        }

        // run preview frame
        self.preview_editor.frame(window, layout.preview, &[0]c.SDL_Event{});
    }
};
