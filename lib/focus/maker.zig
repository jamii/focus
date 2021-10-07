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
    state: union(enum) {
        Choosing,
        Running: struct {
            command: []const u8,
            child_process: *std.ChildProcess,
        },
        Finished: struct {
            command: []const u8,
        },
    },

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
            .state = .Choosing,
        };
        return self;
    }

    pub fn deinit(self: *Maker) void {
        switch (self.state) {
            .Choosing => return,
            .Running => |running| {
                running.child_process.stdout.?.close();
                running.child_process.stderr.?.close();
                running.child_process.deinit();
                self.app.allocator.free(running.command);
            },
            .Finished => |finished| {
                self.app.allocator.free(finished.command);
            },
        }
        self.app.allocator.free(self.history_string);
        self.selector.deinit();
        self.input.deinit();
        const buffer = self.result_editor.buffer;
        self.result_editor.deinit();
        buffer.deinit();
        self.app.allocator.destroy(self);
    }

    pub fn frame(self: *Maker, window: *Window, rect: Rect, events: []const c.SDL_Event) void {
        switch (self.state) {
            .Choosing => {
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
                var command_o: ?[]const u8 = null;
                if (action == .SelectOne and filtered_history.len > 0) {
                    command_o = self.app.dupe(filtered_history[self.selector.selected]);
                }
                if (action == .SelectRaw) {
                    command_o = self.app.dupe(self.input.getText());
                }

                if (command_o) |command| {
                    // add command to history
                    const history_file = std.fs.cwd().openFile("/home/jamie/.bash_history", .{ .write = true }) catch |err|
                        panic("Failed to open bash history: {}", .{err});
                    history_file.seekFromEnd(0) catch |err|
                        panic("Failed to seek to end of bash history: {}", .{err});
                    std.fmt.format(history_file.writer(), "{s}\n", .{command}) catch |err|
                        panic("Failed to write to bash history: {}", .{err});

                    // start running
                    self.state = .{ .Running = .{
                        .command = command,
                        .child_process = spawn(self.app.allocator, command),
                    } };
                }
            },
            .Running => |running| {
                // check if we've finished running yet
                const wait = std.os.waitpid(running.child_process.pid, std.os.linux.WNOHANG);
                if (wait.pid == running.child_process.pid) {
                    // finished, read results
                    for (&[_]std.fs.File{ running.child_process.stdout.?, running.child_process.stderr.? }) |file| {
                        const contents = file.reader().readAllAlloc(self.app.frame_allocator, std.math.maxInt(usize)) catch |err|
                            panic("{} while reading from command: {s}", .{ err, running.command });
                        file.close();
                        self.result_editor.buffer.insert(self.result_editor.buffer.bytes.items.len, contents);
                    }
                    running.child_process.deinit();
                    self.state = .{ .Finished = .{ .command = running.command } };
                }

                // show results in editor
                self.result_editor.frame(window, rect, events);
            },
            .Finished => {
                // show results in editor
                self.result_editor.frame(window, rect, events);
            },
        }
    }

    pub fn handleAfterSave(self: *Maker) void {
        switch (self.state) {
            .Choosing => return,
            .Running => |running| {
                running.child_process.stdout.?.close();
                running.child_process.stderr.?.close();
                running.child_process.deinit();
                self.result_editor.buffer.replace("");
                self.state = .{ .Running = .{
                    .command = running.command,
                    .child_process = spawn(self.app.allocator, running.command),
                } };
            },
            .Finished => |finished| {
                self.result_editor.buffer.replace("");
                self.state = .{ .Running = .{
                    .command = finished.command,
                    .child_process = spawn(self.app.allocator, finished.command),
                } };
            },
        }
    }
};

fn spawn(allocator: *Allocator, command: []const u8) *std.ChildProcess {
    var child_process = std.ChildProcess.init(
        &.{ "bash", "-c", command },
        allocator,
    ) catch |err|
        panic("{} while running command: {s}", .{ err, command });
    child_process.stdin_behavior = .Ignore;
    child_process.stdout_behavior = .Pipe;
    child_process.stderr_behavior = .Pipe;
    child_process.cwd = "/home/jamie";
    child_process.spawn() catch |err|
        panic("{} while running command: {s}", .{ err, command });
    return child_process;
}
