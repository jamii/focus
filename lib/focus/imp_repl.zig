const focus = @import("../focus.zig");
usingnamespace focus.common;
const meta = focus.meta;
const App = focus.App;
const Buffer = focus.Buffer;
const Editor = focus.Editor;
const Window = focus.Window;
const style = focus.style;
const Selector = focus.Selector;
const imp = @import("../../imp/lib/imp.zig");

pub const ImpRepl = struct {
    app: *App,
    result_editor: *Editor,

    program: []const u8,
    result_o: ?[]const u8,

    pub fn init(app: *App) *ImpRepl {
        const empty_buffer = Buffer.initEmpty(app, .Real);
        const result_editor = Editor.init(app, empty_buffer, false, false);

        const self = app.allocator.create(ImpRepl) catch oom();
        self.* = ImpRepl{
            .app = app,
            .result_editor = result_editor,
            .program = "",
            .result_o = null,
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
        if (self.result_o) |result| self.app.allocator.free(result);
        self.result_o = null;
    }

    pub fn frame(self: *ImpRepl, window: *Window, rect: Rect, events: []const c.SDL_Event) void {
        if (self.result_o == null) {
            var arena = ArenaAllocator.init(self.app.allocator);
            defer arena.deinit();
            var error_info: ?imp.lang.InterpretErrorInfo = null;
            const result = imp.lang.interpret(&arena, self.program, &error_info);
            var result_buffer = ArrayList(u8).init(self.app.allocator);
            if (result) |type_and_set|
                type_and_set.dumpInto(&arena.allocator, result_buffer.writer()) catch oom()
            else |err|
                imp.lang.InterpretErrorInfo.dumpInto(error_info, err, result_buffer.writer()) catch oom();
            self.result_o = result_buffer.toOwnedSlice();
            self.result_editor.buffer.replace(self.result_o.?);
        }
        self.result_editor.frame(window, rect, events);
    }
};
