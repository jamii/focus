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
    program_editor: *Editor,
    result_editor: *Editor,
    imp_worker: *imp.lang.Worker,
    last_program_id: usize,
    last_result_id: usize,

    pub fn init(app: *App, program_editor: *Editor) *ImpRepl {
        const empty_buffer = Buffer.initEmpty(app, .Real);
        const result_editor = Editor.init(app, empty_buffer, false, false);
        const imp_worker = imp.lang.Worker.init(app.allocator) catch |err|
            panic("Failed to start imp worker: {}", .{err});

        const self = app.allocator.create(ImpRepl) catch oom();
        self.* = ImpRepl{
            .app = app,
            .program_editor = program_editor,
            .result_editor = result_editor,
            .imp_worker = imp_worker,
            .last_program_id = 0,
            .last_result_id = 0,
        };
        return self;
    }

    pub fn deinit(self: *ImpRepl) void {
        self.imp_worker.deinitSoon();
        // imp_worker will deinit itself when it shuts down
        self.result_editor.deinit();
        self.program_editor.imp_repl_o = null;
        self.app.allocator.destroy(self);
        self.* = undefined;
    }

    // called from program_editor on change
    pub fn setProgram(self: *ImpRepl, program: []const u8, cursor_pos: usize) void {
        self.last_program_id += 1;
        self.imp_worker.setProgram(.{
            .text = program[0..cursor_pos],
            .id = self.last_program_id,
        }) catch oom();
    }

    pub fn frame(self: *ImpRepl, window: *Window, rect: Rect, events: []const c.SDL_Event) void {
        if (self.imp_worker.getResult()) |result| {
            self.result_editor.buffer.replace(result.text);
            self.last_result_id = result.id;
            self.app.allocator.free(result.text);
        }
        self.result_editor.frame(window, rect, events);
        if (self.last_program_id != self.last_result_id)
            window.queueRect(rect, style.fade_color);
    }
};
