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
const mach_compat = focus.mach_compat;

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

        var projects = u.ArrayList(u8).init(app.frame_allocator);
        {
            const file = std.fs.cwd().openFile(focus.config.projects_file_path, .{}) catch |err|
                u.panic("{} while opening {s}", .{ err, focus.config.projects_file_path });
            defer file.close();
            file.reader().readAllArrayList(&projects, std.math.maxInt(usize)) catch |err|
                u.panic("{} while reading {s}", .{ err, focus.config.projects_file_path });
        }

        var paths = u.ArrayList([]const u8).init(app.allocator);
        var projects_iter = std.mem.splitScalar(u8, projects.items, '\n');
        while (projects_iter.next()) |project_untrimmed| {
            const project = std.mem.trim(u8, project_untrimmed, " ");
            if (project.len == 0) continue;
            if (std.mem.startsWith(u8, project, "#")) continue;
            const result = std.process.Child.run(.{
                .allocator = app.frame_allocator,
                .argv = &[_][]const u8{ "rg", "--files", "-0" },
                .cwd = project,
                .max_output_bytes = 128 * 1024 * 1024,
            }) catch |err| u.panic("{} while calling rg", .{err});
            u.assert(result.term == .Exited and result.term.Exited == 0);
            var lines = std.mem.splitScalar(u8, result.stdout, 0);
            while (lines.next()) |line| {
                const path = std.fs.path.join(app.allocator, &[2][]const u8{ project, line }) catch u.oom();
                paths.append(path) catch u.oom();
            }
        }

        std.mem.sort([]const u8, paths.items, {}, struct {
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
            .paths = paths.toOwnedSlice() catch u.oom(),
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

    pub fn frame(self: *ProjectFileOpener, window: *Window, rect: u.Rect, events: []const mach_compat.Event) void {
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
        if (action == .SelectOne) {
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
        if (self.selector.selected >= filtered_paths.len) {
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
        self.preview_editor.frame(window, layout.preview, &[0]mach_compat.Event{});
    }
};
