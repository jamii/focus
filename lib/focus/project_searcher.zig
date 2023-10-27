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
const ChildProcess = focus.ChildProcess;
const mach_compat = focus.mach_compat;

pub const ProjectSearcher = struct {
    app: *App,
    project_dir: []const u8,
    preview_editor: *Editor,
    filter_mode: FilterMode,
    input: SingleLineEditor,
    selector: Selector,
    child_process: ?ChildProcess,

    const FilterMode = enum {
        FixedStrings,
        Regexp,
    };

    pub fn init(app: *App, project_dir: []const u8, init_filter_mode: FilterMode, init_filter: ?[]const u8) *ProjectSearcher {
        const empty_buffer = Buffer.initEmpty(app, .{
            .enable_completions = false,
            .enable_undo = false,
        });
        const preview_editor = Editor.init(app, empty_buffer, .{
            .show_status_bar = false,
            .show_completer = false,
        });
        const input = SingleLineEditor.init(app, init_filter orelse app.last_search_filter);
        input.editor.goRealLineStart(input.editor.getMainCursor());
        input.editor.setMark();
        input.editor.goRealLineEnd(input.editor.getMainCursor());
        var selector = Selector.init(app);
        selector.selected = app.last_project_search_selected;

        const self = app.allocator.create(ProjectSearcher) catch u.oom();
        self.* =
            ProjectSearcher{
            .app = app,
            .project_dir = project_dir,
            .preview_editor = preview_editor,
            .filter_mode = init_filter_mode,
            .input = input,
            .selector = selector,
            .child_process = null,
        };
        self.startRipgrep();
        return self;
    }

    pub fn deinit(self: *ProjectSearcher) void {
        if (self.child_process) |*child_process| child_process.deinit();
        self.selector.deinit();
        self.input.deinit();
        const buffer = self.preview_editor.buffer;
        self.preview_editor.deinit();
        buffer.deinit();
        // TODO should self own project_dir?
        self.app.allocator.destroy(self);
    }

    fn startRipgrep(self: *ProjectSearcher) void {
        const filter = self.input.getText();
        if (filter.len > 0) {
            if (self.child_process) |*child_process| child_process.deinit();
            self.child_process = ChildProcess.init(
                self.app.allocator,
                self.project_dir,
                &[_][]const u8{
                    "rg",
                    "--line-number",
                    "--sort",
                    "path",
                    switch (self.filter_mode) {
                        .FixedStrings => "--fixed-strings",
                        .Regexp => "--regexp",
                    },
                    filter,
                },
            );
        }
    }

    pub fn frame(self: *ProjectSearcher, window: *Window, rect: u.Rect, events: []const mach_compat.Event) void {
        const layout = window.layoutSearcherWithPreview(rect);

        // run input frame
        const input_changed = self.input.frame(window, layout.input, events);

        // maybe start ripgrep
        if (input_changed == .Changed) {
            self.selector.selected = 0;
            self.selector.setTextAndRanges("", &.{});
            if (self.child_process) |*child_process| {
                child_process.deinit();
                self.child_process = null;
            }
            self.startRipgrep();
        }

        // if output changed, update selector ranges
        if (self.child_process) |child_process| {
            const new_text = child_process.read(self.app.frame_allocator);
            if (new_text.len > 0) {
                self.selector.buffer.insert(self.selector.buffer.getBufferEnd(), new_text);
                const text = self.selector.buffer.bytes.items;
                var result_ranges = u.ArrayList([2]usize).init(self.app.frame_allocator);
                var start: usize = 0;
                var end: usize = 0;
                while (end < text.len) {
                    end = std.mem.indexOfPos(u8, text, start, "\n") orelse text.len;
                    if (start != end)
                        result_ranges.append(.{ start, end }) catch u.oom();
                    start = end + 1;
                }
                self.selector.setRanges(result_ranges.toOwnedSlice() catch u.oom());
            }
        }

        // run selector frame
        const action = self.selector.frame(window, layout.selector, events);

        // set cached search text
        self.app.allocator.free(self.app.last_search_filter);
        self.app.last_search_filter = self.app.dupe(self.input.getText());
        self.app.last_project_search_selected = self.selector.selected;

        var line_number: ?usize = null;
        var path: ?[]const u8 = null;
        if (self.selector.selected < self.selector.ranges.len) {
            // deinit old preview
            const buffer = self.preview_editor.buffer;
            self.preview_editor.deinit();
            buffer.deinit();

            // see if we can parse selection
            const range = self.selector.ranges[self.selector.selected];
            const line = self.selector.buffer.bytes.items[range[0]..range[1]];
            var parts = std.mem.split(u8, line, ":");
            if (parts.next()) |path_suffix|
                path = std.fs.path.join(self.app.frame_allocator, &[2][]const u8{ self.project_dir, path_suffix }) catch u.oom();
            if (parts.next()) |line_number_string|
                line_number = std.fmt.parseInt(usize, line_number_string, 10) catch null;

            // init new preview
            const preview_buffer = if (path != null)
                Buffer.initFromAbsoluteFilename(self.app, .{
                    .enable_completions = false,
                    .enable_undo = false,
                }, path.?)
            else
                Buffer.initEmpty(self.app, .{
                    .enable_completions = false,
                    .enable_undo = false,
                });
            self.preview_editor = Editor.init(self.app, preview_buffer, .{
                .show_status_bar = false,
                .show_completer = false,
            });
            if (line_number != null) {
                const cursor = self.preview_editor.getMainCursor();
                self.preview_editor.tryGoRealLine(cursor, line_number.? - 1);
                self.preview_editor.setMark();
                self.preview_editor.goRealLineEnd(cursor);
                self.preview_editor.setCenterAtPos(cursor.head.pos);
            }

            // handle action
            if (action == .SelectOne and path != null and line_number != null) {
                const new_buffer = self.app.getBufferFromAbsoluteFilename(path.?);
                const new_editor = Editor.init(self.app, new_buffer, .{});
                new_editor.top_pixel = self.preview_editor.top_pixel;
                var cursor = new_editor.getMainCursor();
                new_editor.tryGoRealLine(cursor, line_number.? - 1);
                new_editor.setMark();
                new_editor.goRealLineEnd(cursor);
                new_editor.setCenterAtPos(cursor.head.pos);
                window.popView();
                window.pushView(new_editor);
            }
        }

        // run preview frame
        self.preview_editor.frame(window, layout.preview, &[0]mach_compat.Event{});
    }
};
