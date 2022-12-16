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

pub const Launcher = struct {
    app: *App,
    input: SingleLineEditor,
    selector: Selector,
    exes: []const []const u8,

    pub fn init(app: *App) *Launcher {
        const input = SingleLineEditor.init(app, "");
        var selector = Selector.init(app);

        var exes = u.ArrayList([]const u8).init(app.allocator);
        {
            const result = std.ChildProcess.exec(.{
                .allocator = app.frame_allocator,
                .argv = &[_][]const u8{ "fish", "-C", "complete -C ''" },
                .cwd = "/home/jamie",
                .max_output_bytes = 128 * 1024 * 1024,
            }) catch |err| u.panic("{} while calling compgen", .{err});
            u.assert(result.term == .Exited and result.term.Exited == 0);
            var lines = std.mem.split(u8, result.stdout, "\n");
            while (lines.next()) |line| {
                var words = std.mem.split(u8, line, "\t");
                const command = words.next().?;
                exes.append(app.dupe(command)) catch u.oom();
            }
        }

        const self = app.allocator.create(Launcher) catch u.oom();
        self.* = Launcher{
            .app = app,
            .input = input,
            .selector = selector,
            .exes = exes.toOwnedSlice(),
        };
        return self;
    }

    pub fn deinit(self: *Launcher) void {
        for (self.exes) |exe| self.app.allocator.free(exe);
        self.app.allocator.free(self.exes);
        self.selector.deinit();
        self.input.deinit();
        self.app.allocator.destroy(self);
    }

    pub fn frame(self: *Launcher, window: *Window, rect: u.Rect, events: []const mach_compat.Event) void {
        const layout = window.layoutSearcher(rect);

        // run input frame
        const input_changed = self.input.frame(window, layout.input, events);
        if (input_changed == .Changed) self.selector.selected = 0;

        // filter exes
        const filtered_exes = u.fuzzy_search(self.app.frame_allocator, self.exes, self.input.getText());

        // run selector frame
        self.selector.setItems(filtered_exes);
        const action = self.selector.frame(window, layout.selector, events);

        // handle action
        if (action == .SelectOne) {
            const exe = filtered_exes[self.selector.selected];
            // TODO this is kinda hacky :D
            const command = u.format(self.app.frame_allocator, "{s} & disown", .{exe});
            var process = std.ChildProcess.init(
                &[_][]const u8{ "fish", "-c", command },
                self.app.frame_allocator,
            );
            process.spawn() catch |err| u.panic("Failed to spawn {s}: {}", .{ command, err });
            window.close_after_frame = true;
        }
    }
};
