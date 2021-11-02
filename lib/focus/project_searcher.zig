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

pub const ProjectSearcher = struct {
    app: *App,
    project_dir: []const u8,
    preview_editor: *Editor,
    input: SingleLineEditor,
    selector: Selector,

    pub fn init(app: *App, project_dir: []const u8) *ProjectSearcher {
        const empty_buffer = Buffer.initEmpty(app, .Real);
        const preview_editor = Editor.init(app, empty_buffer, false, false);
        const input = SingleLineEditor.init(app, app.last_search_filter);
        input.editor.goRealLineStart(input.editor.getMainCursor());
        input.editor.setMark();
        input.editor.goRealLineEnd(input.editor.getMainCursor());
        var selector = Selector.init(app);
        selector.selected = app.last_project_search_selected;

        const self = app.allocator.create(ProjectSearcher) catch oom();
        self.* =
            ProjectSearcher{
            .app = app,
            .project_dir = project_dir,
            .preview_editor = preview_editor,
            .input = input,
            .selector = selector,
        };
        return self;
    }

    pub fn deinit(self: *ProjectSearcher) void {
        self.selector.deinit();
        self.input.deinit();
        const buffer = self.preview_editor.buffer;
        self.preview_editor.deinit();
        buffer.deinit();
        // TODO should this own project_dir?
        self.app.allocator.destroy(self);
    }

    pub fn frame(self: *ProjectSearcher, window: *Window, rect: Rect, events: []const c.SDL_Event) void {
        const layout = window.layoutSearcherWithPreview(rect);

        // run input frame
        const input_changed = self.input.frame(window, layout.input, events);
        if (input_changed == .Changed) self.selector.selected = 0;

        // get and filter results
        var text: []const u8 = "";
        var ranges = ArrayList([2]usize).init(self.app.frame_allocator);
        {
            const filter = self.input.getText();
            if (filter.len > 0) {
                const result = std.ChildProcess.exec(.{
                    .allocator = self.app.frame_allocator,
                    // TODO would prefer null separated but tricky to parse
                    .argv = &[_][]const u8{ "rg", "--line-number", "--sort", "path", "--fixed-strings", filter },
                    .cwd = self.project_dir,
                    .max_output_bytes = 128 * 1024 * 1024,
                }) catch |err| panic("{} while calling rg", .{err});
                assert(result.term == .Exited); // exits with 1 if no search results
                text = result.stdout;
                var start: usize = 0;
                var end: usize = 0;
                while (end < text.len) {
                    end = std.mem.indexOfPos(u8, text, start, "\n") orelse text.len;
                    ranges.append(.{ start, end }) catch oom();
                    start = end + 1;
                }
            }
        }

        // run selector frame
        const action = self.selector.frameInner(window, layout.selector, events, text, ranges.items);

        // set cached search text
        self.app.allocator.free(self.app.last_search_filter);
        self.app.last_search_filter = self.app.dupe(self.input.getText());
        self.app.last_project_search_selected = self.selector.selected;

        // update preview
        var line_number: ?usize = null;
        var path: ?[]const u8 = null;
        if (ranges.items.len > 0) {
            const range = ranges.items[self.selector.selected];
            const line = text[range[0]..range[1]];
            var parts = std.mem.split(line, ":");
            const path_suffix = parts.next().?;
            const line_number_string = parts.next().?;
            line_number = std.fmt.parseInt(usize, line_number_string, 10) catch |err| panic("{} while parsing line number {s} from rg", .{ err, line_number_string });

            path = std.fs.path.join(self.app.frame_allocator, &[2][]const u8{ self.project_dir, path_suffix }) catch oom();
            const buffer = self.preview_editor.buffer;
            self.preview_editor.deinit();
            buffer.deinit();
            const preview_buffer = Buffer.initFromAbsoluteFilename(self.app, .Real, path.?);
            self.preview_editor = Editor.init(self.app, preview_buffer, false, false);

            {
                const cursor = self.preview_editor.getMainCursor();
                self.preview_editor.goRealLine(cursor, line_number.? - 1);
                self.preview_editor.setMark();
                self.preview_editor.goRealLineEnd(cursor);
                self.preview_editor.setCenterAtPos(cursor.head.pos);
            }
        }

        // run preview frame
        self.preview_editor.frame(window, layout.preview, &[0]c.SDL_Event{});

        // handle action
        if (ranges.items.len > 0 and action == .SelectOne) {
            const new_buffer = self.app.getBufferFromAbsoluteFilename(path.?);
            const new_editor = Editor.init(self.app, new_buffer, true, true);
            new_editor.top_pixel = self.preview_editor.top_pixel;
            var cursor = new_editor.getMainCursor();
            new_editor.goRealLine(cursor, line_number.? - 1);
            new_editor.setMark();
            new_editor.goRealLineEnd(cursor);
            new_editor.setCenterAtPos(cursor.head.pos);
            window.popView();
            window.pushView(new_editor);
        }
    }
};
