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

pub const Launcher = struct {
    app: *App,
    input: SingleLineEditor,
    selector: Selector,
    exes: []const []const u8,

    pub fn init(app: *App) *Launcher {
        const input = SingleLineEditor.init(app, "");
        var selector = Selector.init(app);

        var exes = ArrayList([]const u8).init(app.allocator);
        {
            const result = std.ChildProcess.exec(.{
                .allocator = app.frame_allocator,
                .argv = &[_][]const u8{ "bash", "-c", "compgen -c" },
                .cwd = "/home/jamie",
                .max_output_bytes = 128 * 1024 * 1024,
            }) catch |err| panic("{} while calling compgen", .{err});
            assert(result.term == .Exited and result.term.Exited == 0);
            var lines = std.mem.split(result.stdout, "\n");
            while (lines.next()) |line| {
                exes.append(app.dupe(line)) catch oom();
            }
        }

        const self = app.allocator.create(Launcher) catch oom();
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

    pub fn frame(self: *Launcher, window: *Window, rect: Rect, events: []const c.SDL_Event) void {
        const layout = window.layoutSearcher(rect);

        // run input frame
        self.input.frame(window, layout.input, events);

        // filter exes
        const filtered_exes = fuzzy_search(self.app.frame_allocator, self.exes, self.input.getText());

        // run selector frame
        const action = self.selector.frame(window, layout.selector, events, filtered_exes);

        // handle action
        if (filtered_exes.len > 0 and action == .SelectOne) {
            const exe = filtered_exes[self.selector.selected];
            // TODO this is kinda hacky :D
            const command = format(self.app.frame_allocator, "{} & disown", .{exe});
            var process = std.ChildProcess.init(
                &[_][]const u8{ "bash", "-c", command },
                self.app.frame_allocator,
            ) catch |err| panic("Failed to init {}: {}", .{ command, err });
            process.spawn() catch |err| panic("Failed to spawn {}: {}", .{ command, err });
            window.close_after_frame = true;
        }
    }
};
