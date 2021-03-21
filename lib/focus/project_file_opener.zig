const focus = @import("../focus.zig");
usingnamespace focus.common;
const meta = focus.meta;
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
    "/home/jamie/bluetron/",
    "/home/jamie/blog",
    "/home/jamie/streaming-consistency",
};

pub const ProjectFileOpener = struct {
    app: *App,
    preview_editor: *Editor,
    input: SingleLineEditor,
    selector: Selector,
    paths: []const []const u8,

    pub fn init(app: *App) *ProjectFileOpener {
        const empty_buffer = Buffer.initEmpty(app, .Preview);
        const preview_editor = Editor.init(app, empty_buffer, false, false);
        const input = SingleLineEditor.init(app, "");
        const selector = Selector.init(app);

        var paths = ArrayList([]const u8).init(app.allocator);
        for (projects) |project| {
            const result = std.ChildProcess.exec(.{
                .allocator = app.frame_allocator,
                .argv = &[_][]const u8{ "rg", "--files", "-0" },
                .cwd = project,
                .max_output_bytes = 128 * 1024 * 1024,
            }) catch |err| panic("{} while calling rg", .{err});
            assert(result.term == .Exited and result.term.Exited == 0);
            var lines = std.mem.split(result.stdout, &[1]u8{0});
            while (lines.next()) |line| {
                const path = std.fs.path.join(app.allocator, &[2][]const u8{ project, line }) catch oom();
                paths.append(path) catch oom();
            }
        }

        std.sort.sort([]const u8, paths.items, {}, struct {
            fn lessThan(_: void, a: []const u8, b: []const u8) bool {
                return std.mem.lessThan(u8, a, b);
            }
        }.lessThan);

        const self = app.allocator.create(ProjectFileOpener) catch oom();
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

    pub fn frame(self: *ProjectFileOpener, window: *Window, rect: Rect, events: []const c.SDL_Event) void {
        const layout = window.layoutSearcherWithPreview(rect);

        // run input frame
        self.input.frame(window, layout.input, events);

        // filter paths
        const filtered_paths = fuzzy_search(self.app.frame_allocator, self.paths, self.input.getText());

        // run selector frame
        const action = self.selector.frame(window, layout.selector, events, filtered_paths);

        // maybe open file
        if (action == .SelectOne and filtered_paths.len > 0) {
            const path = filtered_paths[self.selector.selected];
            if (path.len > 0 and std.fs.path.isSep(path[path.len - 1])) {
                self.input.setText(path);
            } else {
                const new_buffer = self.app.getBufferFromAbsoluteFilename(path);
                const new_editor = Editor.init(self.app, new_buffer, true, true);
                window.popView();
                window.pushView(new_editor);
            }
        }

        // update preview
        const buffer = self.preview_editor.buffer;
        self.preview_editor.deinit();
        buffer.deinit();
        if (filtered_paths.len == 0) {
            const empty_buffer = Buffer.initEmpty(self.app, .Preview);
            self.preview_editor = Editor.init(self.app, empty_buffer, false, false);
        } else {
            const selected = filtered_paths[self.selector.selected];
            if (std.mem.endsWith(u8, selected, "/")) {
                const empty_buffer = Buffer.initEmpty(self.app, .Preview);
                self.preview_editor = Editor.init(self.app, empty_buffer, false, false);
            } else {
                const preview_buffer = Buffer.initFromAbsoluteFilename(self.app, .Preview, selected);
                self.preview_editor = Editor.init(self.app, preview_buffer, false, false);
            }
        }

        // run preview frame
        self.preview_editor.frame(window, layout.preview, &[0]c.SDL_Event{});
    }
};
