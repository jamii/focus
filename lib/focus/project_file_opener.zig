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

const projects = [6][]const u8{
    "/home/jamie/exo/",
    "/home/jamie/exo-secret/",
    "/home/jamie/imp/",
    "/home/jamie/focus/",
    "/home/jamie/tower/",
    "/home/jamie/zig/",
};

pub const ProjectFileOpener = struct {
    app: *App,
    preview_buffer_id: Id,
    preview_editor_id: Id,
    input: SingleLineEditor,
    selector: Selector,
    paths: []const []const u8,

    pub fn init(app: *App) Id {
        const preview_buffer_id = Buffer.initEmpty(app);
        const preview_editor_id = Editor.init(app, preview_buffer_id);
        const input = SingleLineEditor.init(app, "");
        const selector = Selector.init(app);

        var paths = ArrayList([]const u8).init(app.allocator);
        for (projects) |project| {
            const result = std.ChildProcess.exec(.{
                .allocator = app.frame_allocator,
                .argv = &[3][]const u8{ "rg", "--files", "-0" },
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

        var self = ProjectFileOpener{
            .app = app,
            .preview_buffer_id = preview_buffer_id,
            .preview_editor_id = preview_editor_id,
            .input = input,
            .selector = selector,
            .paths = paths.toOwnedSlice(),
        };

        return app.putThing(self);
    }

    pub fn deinit(self: *ProjectFileOpener) void {
        for (self.paths) |completion| {
            self.app.allocator.free(completion);
        }
        self.app.allocator.free(self.paths);
    }

    pub fn frame(self: *ProjectFileOpener, window: *Window, rect: Rect, events: []const c.SDL_Event) void {
        const layout = window.layoutSearcher(rect);

        // run input frame
        self.input.frame(window, layout.input, events);

        // filter paths
        const ScoredPath = struct { score: ?usize, path: []const u8 };
        var scored_paths = ArrayList(ScoredPath).init(self.app.frame_allocator);
        {
            const filter = self.input.getText();
            for (self.paths) |path| {
                if (filter.len > 0) {
                    // TODO find shortest match, not first
                    if (std.mem.indexOfScalar(u8, path, filter[0])) |start| {
                        var is_match = true;
                        var end = start;
                        for (filter[1..]) |char| {
                            if (std.mem.indexOfScalarPos(u8, path, end, char)) |new_end| {
                                end = new_end + 1;
                            } else {
                                is_match = false;
                                break;
                            }
                        }
                        if (is_match) {
                            const score = end - start;
                            scored_paths.append(.{ .score = score, .path = path }) catch oom();
                        }
                    }
                } else {
                    const score = 0;
                    scored_paths.append(.{ .score = score, .path = path }) catch oom();
                }
            }
            std.sort.sort(ScoredPath, scored_paths.items, struct {
                fn lessThan(a: ScoredPath, b: ScoredPath) bool {
                    return meta.deepCompare(a, b) == .LessThan;
                }
            }.lessThan);
        }

        // run selector frame
        var just_paths = ArrayList([]const u8).init(self.app.frame_allocator);
        for (scored_paths.items) |scored_path| just_paths.append(scored_path.path) catch oom();
        const action = self.selector.frame(window, layout.selector, events, just_paths.items);

        // maybe open file
        if (action == .SelectOne and just_paths.items.len > 0) {
            const path = just_paths.items[self.selector.selected];
            if (path.len > 0 and std.fs.path.isSep(path[path.len - 1])) {
                self.input.setText(path);
            } else {
                const new_buffer_id = Buffer.initFromAbsoluteFilename(self.app, path);
                const new_editor_id = Editor.init(self.app, new_buffer_id);
                window.popView();
                window.pushView(new_editor_id);
            }
        }

        // update preview
        var preview_buffer = self.app.getThing(self.preview_buffer_id).Buffer;
        var preview_editor = self.app.getThing(self.preview_editor_id).Editor;
        if (just_paths.items.len == 0) {
            preview_buffer.bytes.shrink(0);
        } else {
            const selected = just_paths.items[self.selector.selected];
            if (std.mem.endsWith(u8, selected, "/")) {
                preview_buffer.bytes.shrink(0);
            } else {
                preview_buffer.load(selected);
            }
        }

        // run preview frame
        preview_editor.frame(window, layout.preview, &[0]c.SDL_Event{});
    }
};
