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
    buffer: *Buffer,
    result_editor: *Editor,

    // mutex protects these fields
    mutex: std.Thread.Mutex,
    // the background thread must check this before setting new_program or new_result
    background_loop_should_stop: bool,
    // new_program and new_result behave like queues, except that they only care about the most recent item
    new_program: ?[]const u8,
    new_result: ?[]const u8,
    // does the result currently being displayed match the program in the buffer
    is_synced: bool,

    pub fn init(app: *App, buffer: *Buffer) *ImpRepl {
        const empty_buffer = Buffer.initEmpty(app, .Real);
        const result_editor = Editor.init(app, empty_buffer, false, false);

        const self = app.allocator.create(ImpRepl) catch oom();
        self.* = ImpRepl{
            .app = app,
            .buffer = buffer,
            .result_editor = result_editor,
            .mutex = std.Thread.Mutex{},
            .new_program = null,
            .new_result = null,
            .background_loop_should_stop = false,
            .is_synced = true,
        };

        _ = std.Thread.spawn(.{}, ImpRepl.backgroundLoop, .{self}) catch |err|
            panic("Failed to spawn background thread: {}", .{err});

        return self;
    }

    pub fn deinit(self: *ImpRepl) void {
        {
            const held = self.mutex.acquire();
            defer held.release();
            if (self.new_program) |program| self.app.allocator.free(program);
            if (self.new_result) |result| self.app.allocator.free(result);
            self.background_loop_should_stop = true;
        }
        self.result_editor.deinit();
        self.buffer.imp_repl_o = null;
        self.app.allocator.destroy(self);
        // TODO if background thread tries to acquire self.mutex at this point, it might panic
    }

    // called from Buffer on change
    pub fn setProgram(self: *ImpRepl, program: []const u8) void {
        const held = self.mutex.acquire();
        defer held.release();
        if (self.new_program) |old_program| self.app.allocator.free(old_program);
        self.new_program = self.app.dupe(program);
        self.is_synced = false;
    }

    pub fn backgroundLoop(self: *ImpRepl) void {
        while (true) {
            var new_program: ?[]const u8 = null;
            defer if (new_program) |program| self.app.allocator.free(program);

            // see if we have a new program
            {
                const held = self.mutex.acquire();
                defer held.release();
                if (self.background_loop_should_stop) return;
                if (self.new_program) |program| {
                    new_program = program;
                    self.new_program = null;
                }
            }

            // if so, produce a new result
            if (new_program) |program| {
                // eval
                var arena = ArenaAllocator.init(self.app.allocator);
                defer arena.deinit();
                var error_info: ?imp.lang.InterpretErrorInfo = null;
                const result = imp.lang.interpret(&arena, program, &error_info);

                // print result
                var result_buffer = ArrayList(u8).init(self.app.allocator);
                defer result_buffer.deinit();
                if (result) |type_and_set|
                    type_and_set.dumpInto(&arena.allocator, result_buffer.writer()) catch oom()
                else |err|
                    imp.lang.InterpretErrorInfo.dumpInto(error_info, err, result_buffer.writer()) catch oom();

                // set result
                {
                    const held = self.mutex.acquire();
                    defer held.release();
                    if (self.background_loop_should_stop) return;
                    if (self.new_result) |old_result| self.app.allocator.free(old_result);
                    self.new_result = result_buffer.toOwnedSlice();
                    if (self.new_program == null) self.is_synced = true;
                }
            }
        }
    }

    pub fn frame(self: *ImpRepl, window: *Window, rect: Rect, events: []const c.SDL_Event) void {
        {
            const held = self.mutex.acquire();
            defer held.release();
            if (self.new_result) |result| {
                self.result_editor.buffer.replace(result);
                self.app.allocator.free(result);
                self.new_result = null;
            }
        }
        self.result_editor.frame(window, rect, events);
        if (!self.is_synced)
            window.queueRect(rect, style.highlight_color);
    }
};
