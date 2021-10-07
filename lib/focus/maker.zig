const focus = @import("../focus.zig");
usingnamespace focus.common;
const App = focus.App;
const Buffer = focus.Buffer;
const Editor = focus.Editor;
const Window = focus.Window;
const style = focus.style;
const SingleLineEditor = focus.SingleLineEditor;
const Selector = focus.Selector;

pub const Maker = struct {
    app: *App,
    input: SingleLineEditor,
    selector: Selector,
    result_editor: *Editor,
    history_string: []const u8,
    history: []const []const u8,
    command: ?[]const u8,

    pub fn init(app: *App) *Maker {
        const empty_buffer = Buffer.initEmpty(app, .Real);
        const result_editor = Editor.init(app, empty_buffer, false, false);
        const input = SingleLineEditor.init(app, "");
        const selector = Selector.init(app);
        const history_file = std.fs.cwd().openFile("/home/jamie/.bash_history", .{}) catch |err|
            panic("Failed to open bash history: {}", .{err});
        const history_string = history_file.reader().readAllAlloc(app.allocator, std.math.maxInt(usize)) catch |err|
            panic("Failed to read bash history: {}", .{err});
        var history_lines = std.mem.split(history_string, "\n");
        var history = ArrayList([]const u8).init(app.allocator);
        while (history_lines.next()) |line|
            if (line.len != 0)
                history.append(line) catch oom();
        std.mem.reverse([]const u8, history.items);
        const self = app.allocator.create(Maker) catch oom();
        self.* = Maker{
            .app = app,
            .input = input,
            .selector = selector,
            .result_editor = result_editor,
            .history_string = history_string,
            .history = history.toOwnedSlice(),
            .command = null,
        };
        return self;
    }

    pub fn deinit(self: *Maker) void {
        if (self.command) |command| self.app.allocator.free(command);
        self.app.allocator.free(self.history_string);
        self.selector.deinit();
        self.input.deinit();
        const buffer = self.result_editor.buffer;
        self.result_editor.deinit();
        buffer.deinit();
        self.app.allocator.destroy(self);
    }

    pub fn frame(self: *Maker, window: *Window, rect: Rect, events: []const c.SDL_Event) void {
        if (self.command) |command| {
            // run command
            const result = std.ChildProcess.exec(.{
                .allocator = self.app.frame_allocator,
                // TODO would prefer null separated but tricky to parse
                .argv = &[_][]const u8{ "bash", "-c", command },
                .cwd = "/home/jamie",
                .max_output_bytes = 128 * 1024 * 1024,
            }) catch |err| panic("{} while calling running command: {s}", .{ err, command });
            assert(result.term == .Exited);

            // show results in editor
            self.result_editor.buffer.replace(result.stdout);
            self.result_editor.buffer.insert(result.stdout.len, result.stderr);
            self.result_editor.frame(window, rect, events);
        } else {
            const layout = window.layoutSearcher(rect);

            // run input frame
            const input_changed = self.input.frame(window, layout.input, events);
            if (input_changed == .Changed) self.selector.selected = 0;

            // filter lines
            const filtered_history = fuzzy_search(self.app.frame_allocator, self.history, self.input.getText());

            // run selector frame
            var action: Selector.Action = .None;
            action = self.selector.frame(window, layout.selector, events, filtered_history);

            // maybe run command
            if (action == .SelectOne and filtered_history.len > 0) {
                self.command = self.app.dupe(filtered_history[self.selector.selected]);
            }
            if (action == .SelectRaw) {
                self.command = self.app.dupe(self.input.getText());
            }

            // add command to history
            if (self.command) |command| {
                const history_file = std.fs.cwd().openFile("/home/jamie/.bash_history", .{ .write = true }) catch |err|
                    panic("Failed to open bash history: {}", .{err});
                history_file.seekFromEnd(0) catch |err|
                    panic("Failed to seek to end of bash history: {}", .{err});
                std.fmt.format(history_file.writer(), "{s}\n", .{command}) catch |err|
                    panic("Failed to write to bash history: {}", .{err});
            }
        }
    }
};
