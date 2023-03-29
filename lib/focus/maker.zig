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
const ErrorLister = focus.ErrorLister;
const ChildProcess = focus.ChildProcess;
const mach_compat = focus.mach_compat;

pub const Maker = struct {
    app: *App,
    input: SingleLineEditor,
    selector: Selector,
    result_editor: *Editor,
    history_string: []const u8,
    history: []const []const u8,
    state: union(enum) {
        ChoosingDir,
        ChoosingCommand: struct {
            dirname: []const u8,
        },
        Running: struct {
            dirname: []const u8,
            command: []const u8,
            child_process: ChildProcess,
            error_locations: u.ArrayList(ErrorLister.ErrorLocation),

            const Self = @This();
            fn clearErrorLocations(self: *Self, allocator: u.Allocator) void {
                for (self.error_locations.items) |error_location|
                    error_location.deinit(allocator);
                self.error_locations.shrinkRetainingCapacity(0);
            }
            fn deinit(self: *Self, allocator: u.Allocator) void {
                for (self.error_locations.items) |error_location|
                    error_location.deinit(allocator);
                self.error_locations.deinit();
                self.child_process.deinit();
                allocator.free(self.command);
                allocator.free(self.dirname);
            }
        },
    },

    pub fn init(app: *App) *Maker {
        const empty_buffer = Buffer.initEmpty(app, .{
            .enable_completions = false,
            .enable_undo = false,
        });
        const result_editor = Editor.init(app, empty_buffer, .{
            .show_status_bar = false,
            .show_completer = false,
        });
        const input = SingleLineEditor.init(app, focus.config.home_path);
        const selector = Selector.init(app);

        const result = std.ChildProcess.exec(.{
            .allocator = app.frame_allocator,
            .argv = &[_][]const u8{ "fish", "--command", "history" },
            .cwd = focus.config.home_path,
            .max_output_bytes = 128 * 1024 * 1024,
        }) catch |err| u.panic("{} while calling fish history", .{err});
        u.assert(result.term == .Exited and result.term.Exited == 0);
        const history_string = app.dupe(result.stdout);
        var history = u.ArrayList([]const u8).init(app.allocator);
        var lines = std.mem.split(u8, history_string, "\n");
        while (lines.next()) |line| {
            history.append(app.dupe(line)) catch u.oom();
        }

        const self = app.allocator.create(Maker) catch u.oom();
        self.* = Maker{
            .app = app,
            .input = input,
            .selector = selector,
            .result_editor = result_editor,
            .history_string = history_string,
            .history = history.toOwnedSlice() catch u.oom(),
            .state = .ChoosingDir,
        };
        return self;
    }

    pub fn deinit(self: *Maker) void {
        switch (self.state) {
            .ChoosingDir => return,
            .ChoosingCommand => |choosing_command| self.app.allocator.free(choosing_command.dirname),
            .Running => |*running| running.deinit(self.app.allocator),
        }
        self.app.allocator.free(self.history);
        self.app.allocator.free(self.history_string);
        self.selector.deinit();
        self.input.deinit();
        const buffer = self.result_editor.buffer;
        self.result_editor.deinit();
        buffer.deinit();
        self.app.allocator.destroy(self);
    }

    pub fn frame(self: *Maker, window: *Window, rect: u.Rect, events: []const mach_compat.Event) void {
        switch (self.state) {
            .ChoosingDir => {
                const layout = window.layoutSearcher(rect);

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
                const dirname = if (path.len > 0 and std.fs.path.isSep(path[path.len - 1]))
                    path[0 .. path.len - 1]
                else
                    std.fs.path.dirname(path) orelse "";

                // maybe enter dir
                if (action == .SelectOne) {
                    self.input.buffer.replace(std.fs.path.join(self.app.frame_allocator, &[_][]const u8{ dirname, results[self.selector.selected] }) catch u.oom());
                    const cursor = self.input.editor.getMainCursor();
                    self.input.editor.goRealLineEnd(cursor);
                }
                // maybe choose current dir
                if (action == .SelectRaw) {
                    const chosen_dirname: []const u8 = self.input.getText();
                    if (chosen_dirname.len > 0 and std.fs.path.isSep(chosen_dirname[chosen_dirname.len - 1])) {
                        self.input.buffer.replace("");
                        self.state = .{ .ChoosingCommand = .{
                            .dirname = self.app.dupe(chosen_dirname),
                        } };
                    }
                }
            },
            .ChoosingCommand => |choosing_command| {
                const layout = window.layoutSearcher(rect);

                // run input frame
                const input_changed = self.input.frame(window, layout.input, events);
                if (input_changed == .Changed) self.selector.selected = 0;

                // filter lines
                const filtered_history = u.fuzzy_search(self.app.frame_allocator, self.history, self.input.getText());

                // run selector frame
                var action: Selector.Action = .None;
                self.selector.setItems(filtered_history);
                action = self.selector.frame(window, layout.selector, events);

                // maybe run command
                var command_o: ?[]const u8 = null;
                if (action == .SelectOne) {
                    command_o = self.app.dupe(filtered_history[self.selector.selected]);
                }
                if (action == .SelectRaw) {
                    command_o = self.app.dupe(self.input.getText());
                }

                if (command_o) |command| {
                    // add command to history
                    const history_path = std.fmt.allocPrint(
                        self.app.frame_allocator,
                        "{s}/.local/share/fish/fish_history",
                        .{focus.config.home_path},
                    ) catch u.oom();
                    const history_file = std.fs.cwd().openFile(history_path, .{ .mode = .write_only }) catch |err|
                        u.panic("Failed to open fish history: {}", .{err});
                    history_file.seekFromEnd(0) catch |err|
                        u.panic("Failed to seek to end of fish history: {}", .{err});
                    std.fmt.format(history_file.writer(), "\n- cmd: {s}\n  when: {}", .{ command, std.time.timestamp() }) catch |err|
                        u.panic("Failed to write to fish history: {}", .{err});
                    history_file.close();

                    // start running
                    self.state = .{ .Running = .{
                        .dirname = choosing_command.dirname,
                        .command = command,
                        .child_process = ChildProcess.init(
                            self.app.allocator,
                            choosing_command.dirname,
                            &.{ "fish", "--command", command },
                        ),
                        .error_locations = u.ArrayList(ErrorLister.ErrorLocation).init(self.app.allocator),
                    } };
                }
            },
            .Running => |*running| {
                const new_text = running.child_process.read(self.app.frame_allocator);
                if (new_text.len > 0) {
                    self.result_editor.buffer.insert(self.result_editor.buffer.getBufferEnd(), new_text);
                    // parse results
                    running.clearErrorLocations(self.app.allocator);
                    const error_locations = &running.error_locations;
                    const text = self.result_editor.buffer.bytes.items;
                    var i: usize = 0;
                    next_match: while (i < text.len) {
                        // (whitespace|":")!+ ":"
                        const path_start = i;
                        while (i < text.len) {
                            switch (text[i]) {
                                ' ', '\t', '\r', '\n', ':' => {
                                    break;
                                },
                                else => {
                                    i += 1;
                                },
                            }
                        }
                        const path_end = i;
                        if (path_start == path_end) {
                            i += 1;
                            continue :next_match;
                        }

                        if (i < text.len and text[i] == ':')
                            i += 1
                        else
                            continue :next_match;

                        // digit+ :
                        const line_start = i;
                        while (i < text.len) {
                            switch (text[i]) {
                                '0'...'9' => {
                                    i += 1;
                                },
                                else => {
                                    break;
                                },
                            }
                        }
                        const line_end = i;
                        if (line_start == line_end) {
                            continue :next_match;
                        }

                        // (":" digit+)?
                        var col_start: ?usize = null;
                        var col_end: ?usize = null;
                        if (i < text.len and text[i] == ':') {
                            i += 1;
                            col_start = i;
                            while (i < text.len) {
                                switch (text[i]) {
                                    '0'...'9' => {
                                        i += 1;
                                    },
                                    else => {
                                        break;
                                    },
                                }
                            }
                            col_end = i;
                            if (col_start.? == col_end.?) {
                                continue :next_match;
                            }
                        }

                        const path = text[path_start..path_end];
                        const full_path = std.fs.path.resolve(self.app.allocator, &.{
                            running.dirname,
                            path,
                        }) catch continue :next_match;

                        const line = std.fmt.parseInt(
                            usize,
                            text[line_start..line_end],
                            10,
                        ) catch continue :next_match;

                        const col = if (col_start == null)
                            1
                        else
                            std.fmt.parseInt(
                                usize,
                                text[col_start.?..col_end.?],
                                10,
                            ) catch continue :next_match;

                        error_locations.append(.{
                            .report_buffer = self.result_editor.buffer,
                            .report_location = .{ path_start, i },
                            .path = full_path,
                            .line = line,
                            .col = col,
                        }) catch u.oom();
                    }
                }

                // show results in editor
                self.result_editor.frame(window, rect, events);
            },
        }
    }

    pub fn handleAfterSave(self: *Maker) void {
        switch (self.state) {
            .ChoosingDir, .ChoosingCommand => return,
            .Running => |*running| {
                self.result_editor.buffer.replace("");
                running.clearErrorLocations(self.app.allocator);
                running.child_process.deinit();
                running.child_process = ChildProcess.init(
                    self.app.allocator,
                    running.dirname,
                    &.{ "fish", "--command", running.command },
                );
            },
        }
    }
};
