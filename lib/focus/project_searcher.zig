const focus = @import("../focus.zig");
usingnamespace focus.common;
const meta = focus.meta;
const App = focus.App;
const Id = focus.Id;
const Buffer = focus.Buffer;
const Editor = focus.Editor;
const SingleLineEditor = focus.SingleLineEditor;
const Window = focus.Window;
const style = focus.style;
const Selector = focus.Selector;

pub const ProjectSearcher = struct {
    app: *App,
    project_dir: []const u8,
    preview_buffer_id: Id,
    preview_editor_id: Id,
    input: SingleLineEditor,
    selector: Selector,

    pub fn init(app: *App, project_dir: []const u8, init_filter: []const u8) Id {
        const preview_buffer_id = Buffer.initEmpty(app);
        const preview_editor_id = Editor.init(app, preview_buffer_id, false);
        const input = SingleLineEditor.init(app, init_filter);
        const selector = Selector.init(app);

        return app.putThing(ProjectSearcher{
            .app = app,
            .project_dir = project_dir,
            .preview_buffer_id = preview_buffer_id,
            .preview_editor_id = preview_editor_id,
            .input = input,
            .selector = selector,
        });
    }

    pub fn deinit(self: *ProjectSearcher) void {
        self.selector.deinit();
        self.input.deinit();
    }

    pub fn frame(self: *ProjectSearcher, window: *Window, rect: Rect, events: []const c.SDL_Event) void {
        const layout = window.layoutSearcher(rect);

        // run input frame
        self.input.frame(window, layout.input, events);

        // get and filter results
        var results = ArrayList([]const u8).init(self.app.frame_allocator);
        {
            const filter = self.input.getText();
            if (filter.len > 0) {
                const result = std.ChildProcess.exec(.{
                    .allocator = self.app.frame_allocator,
                    // TODO would prefer null separated but tricky to parse
                    .argv = &[6][]const u8{ "rg", "--line-number", "--sort", "path", "--fixed-strings", filter },
                    .cwd = self.project_dir,
                    .max_output_bytes = 128 * 1024 * 1024,
                }) catch |err| panic("{} while calling rg", .{err});
                assert(result.term == .Exited); // exits with 1 if no search results
                var lines = std.mem.split(result.stdout, "\n");
                while (lines.next()) |line| {
                    if (line.len != 0) results.append(line) catch oom();
                }
            }
        }

        // run selector frame
        const action = self.selector.frame(window, layout.selector, events, results.items);

        // update preview
        var preview_buffer = self.app.getThing(self.preview_buffer_id).Buffer;
        var preview_editor = self.app.getThing(self.preview_editor_id).Editor;
        preview_editor.collapseCursors();
        preview_editor.clearMark();
        if (results.items.len > 0) {
            const line = results.items[self.selector.selected];
            var parts = std.mem.split(line, ":");
            const path_suffix = parts.next().?;
            const line_number_string = parts.next().?;
            const line_number = std.fmt.parseInt(usize, line_number_string, 10) catch |err| panic("{} while parsing line number {s} from rg", .{ err, line_number_string });

            const path = std.fs.path.join(self.app.frame_allocator, &[2][]const u8{ self.project_dir, path_suffix }) catch oom();
            preview_buffer.load(path);

            var cursor = preview_editor.getMainCursor();
            preview_editor.goLine(cursor, line_number - 1);
            preview_editor.setMark();
            preview_editor.goLineEnd(cursor);
            // TODO centre cursor

            if (action == .SelectOne) {
                window.popView();
                window.pushView(self.preview_editor_id);
            }
        }

        // run preview frame
        preview_editor.frame(window, layout.preview, &[0]c.SDL_Event{});
    }
};
