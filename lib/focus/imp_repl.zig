const focus = @import("../focus.zig");
usingnamespace focus.common;
const meta = focus.meta;
const App = focus.App;
const Buffer = focus.Buffer;
const Editor = focus.Editor;
const Window = focus.Window;
const style = focus.style;
const Selector = focus.Selector;

pub const ImpRepl = struct {
    app: *App,
    result_editor: *Editor,

    program: []const u8,
    result: ?usize,

    pub fn init(app: *App) *ImpRepl {
        const empty_buffer = Buffer.initEmpty(app, .Real);
        const result_editor = Editor.init(app, empty_buffer, false, false);

        const self = app.allocator.create(ImpRepl) catch oom();
        self.* = ImpRepl{
            .app = app,
            .result_editor = result_editor,
            .program = "",
            .result = null,
        };
        return self;
    }

    pub fn deinit(self: *ImpRepl) void {
        self.result_editor.deinit();
        self.app.allocator.destroy(self);
    }

    pub fn setProgram(self: *ImpRepl, program: []const u8) void {
        self.app.allocator.free(self.program);
        self.program = self.app.dupe(program);
        self.result = null;
    }

    pub fn frame(self: *ImpRepl, window: *Window, rect: Rect, events: []const c.SDL_Event) void {
        if (self.result == null) {
            self.result = self.program.len;
        }
        const result = format(self.app.frame_allocator, "{}", .{self.result});
        self.result_editor.buffer.replace(result);
        self.result_editor.frame(window, rect, events);
    }
};
