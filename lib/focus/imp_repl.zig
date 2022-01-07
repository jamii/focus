const std = @import("std");
const focus = @import("../focus.zig");
const u = focus.util;
const c = focus.util.c;
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
    last_response: imp.lang.Worker.Response,

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
            u.panic("Failed to start imp worker: {}", .{err});

        const self = app.allocator.create(ImpRepl) catch u.oom();
        self.* = ImpRepl{
            .app = app,
            .program_editor = program_editor,
            .result_editor = result_editor,
            .imp_worker = imp_worker,
            .last_request_id = 0,
            .last_response = .{
                .id = 0,
                .text = "",
                .kind = .{ .Ok = null },
                .warnings = &.{},
            },
        };
        return self;
    }

    pub fn deinit(self: *ImpRepl) void {
        // imp_worker will deinit itself when it shuts down
        self.imp_worker.deinitSoon();
        self.last_response.deinit(self.app.allocator);
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
        }) catch u.oom();
    }

    pub fn frame(self: *ImpRepl, window: *Window, rect: u.Rect, events: []const c.SDL_Event) void {
        if (self.imp_worker.getResponse()) |response| {
            self.last_response.deinit(self.app.allocator);
            self.last_response = response;
            self.result_editor.buffer.replace(response.text);
            self.result_editor.buffer.language = switch (response.kind) {
                .Ok => .Imp,
                .Err => .Unknown,
            };
        }
        self.result_editor.frame(window, rect, events);
        if (self.last_request_id != self.last_response.id)
            window.queueRect(rect, style.fade_color);
    }
};
