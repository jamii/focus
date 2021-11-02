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
    last_request_id: usize,
    last_response_id: usize,
    last_response_kind: imp.lang.Worker.ResponseKind,

    pub fn init(app: *App, program_editor: *Editor) *ImpRepl {
        const empty_buffer = Buffer.initEmpty(app, .{
            .enable_completions = false,
            .enable_undo = false,
        });
        const result_editor = Editor.init(app, empty_buffer, .{
            .show_status_bar = false,
            .show_completer = false,
        });
        const imp_worker = imp.lang.Worker.init(
            app.allocator,
            .{
                .memory_limit_bytes = 1024 * 1024 * 1024,
            },
        ) catch |err|
            panic("Failed to start imp worker: {}", .{err});

        const self = app.allocator.create(ImpRepl) catch oom();
        self.* = ImpRepl{
            .app = app,
            .program_editor = program_editor,
            .result_editor = result_editor,
            .imp_worker = imp_worker,
            .last_request_id = 0,
            .last_response_id = 0,
            .last_response_kind = .{ .Ok = null },
        };
        return self;
    }

    pub fn deinit(self: *ImpRepl) void {
        self.imp_worker.deinitSoon();
        // imp_worker will deinit itself when it shuts down
        self.result_editor.deinit();
        self.program_editor.imp_repl_o = null;
        self.app.allocator.destroy(self);
    }

    // called from program_editor on change
    pub fn setProgram(self: *ImpRepl, program: []const u8, selection: imp.lang.SourceSelection) void {
        self.last_request_id += 1;
        self.imp_worker.setRequest(.{
            .id = self.last_request_id,
            .text = program,
            .selection = selection,
        }) catch oom();
    }

    pub fn frame(self: *ImpRepl, window: *Window, rect: Rect, events: []const c.SDL_Event) void {
        if (self.imp_worker.getResponse()) |response| {
            self.result_editor.buffer.replace(response.text);
            self.last_response_id = response.id;
            self.app.allocator.free(response.text);
            self.last_response_kind = response.kind;
            self.result_editor.buffer.language = switch (response.kind) {
                .Ok => .Imp,
                .Err => .Unknown,
            };
        }
        self.result_editor.frame(window, rect, events);
        if (self.last_request_id != self.last_response_id)
            window.queueRect(rect, style.fade_color);
    }
};
