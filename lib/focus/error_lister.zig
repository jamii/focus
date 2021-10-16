const focus = @import("../focus.zig");
usingnamespace focus.common;
const App = focus.App;
const Buffer = focus.Buffer;
const Editor = focus.Editor;
const Window = focus.Window;
const style = focus.style;
const Selector = focus.Selector;

pub const ErrorLister = struct {
    app: *App,
    error_source_editor: *Editor,
    error_report_editor: *Editor,
    selector: Selector,

    pub fn init(app: *App) *ErrorLister {
        const error_source_buffer = Buffer.initEmpty(app, .Preview);
        const error_source_editor = Editor.init(app, error_source_buffer, false, false);
        // TODO error_report_buffer will get leaked
        const error_report_buffer = Buffer.initEmpty(app, .Real);
        const error_report_editor = Editor.init(app, error_report_buffer, false, false);
        var selector = Selector.init(app);
        selector.selected = app.last_error_lister_selected;
        const self = app.allocator.create(ErrorLister) catch oom();
        self.* = ErrorLister{
            .app = app,
            .selector = selector,
            .error_report_editor = error_report_editor,
            .error_source_editor = error_source_editor,
        };
        return self;
    }

    pub fn deinit(self: *ErrorLister) void {
        self.error_report_editor.deinit();
        const buffer = self.error_source_editor.buffer;
        self.error_source_editor.deinit();
        buffer.deinit();
        self.selector.deinit();
        self.app.allocator.destroy(self);
    }

    pub const ErrorLocation = struct {
        report_buffer: *Buffer,
        report_location: [2]usize,
        path: []const u8,
        line: usize,
        col: usize,

        pub fn deinit(self: ErrorLocation, allocator: *Allocator) void {
            allocator.free(self.path);
        }
    };

    pub fn frame(self: *ErrorLister, window: *Window, rect: Rect, events: []const c.SDL_Event) void {
        const layout = window.layoutLister(rect);

        // get error locations
        var error_locations = ArrayList(ErrorLocation).init(self.app.frame_allocator);
        for (self.app.windows.items) |other_window| {
            if (other_window.getTopView()) |view| {
                switch (view) {
                    .Maker => |maker| switch (maker.state) {
                        .Finished => |finished| error_locations.appendSlice(finished.error_locations) catch oom(),
                        else => {},
                    },
                    else => {},
                }
            }
        }

        // run selector logic only, no rendering
        const action = self.selector.logic(events, error_locations.items.len);

        // set cache selection
        self.app.last_error_lister_selected = self.selector.selected;

        if (error_locations.items.len > 0) {
            const error_location = error_locations.items[self.selector.selected];

            // maybe open file
            if (action == .SelectOne) {
                const new_buffer = self.app.getBufferFromAbsoluteFilename(error_location.path);
                const new_editor = Editor.init(self.app, new_buffer, true, true);
                const cursor = new_editor.getMainCursor();
                new_editor.goRealLineCol(cursor, error_location.line - 1, error_location.col - 1);
                new_editor.setCenterAtPos(cursor.head.pos);
                window.popView();
                window.pushView(new_editor);
            }

            // show report
            self.error_report_editor.deinit();
            self.error_report_editor = Editor.init(self.app, error_location.report_buffer, false, false);
            {
                const cursor = self.error_report_editor.getMainCursor();
                self.error_report_editor.goPos(cursor, error_location.report_location[0]);
                self.error_report_editor.setMark();
                self.error_report_editor.goPos(cursor, error_location.report_location[1]);
                self.error_report_editor.setCenterAtPos(error_location.report_location[0]);
                self.error_report_editor.frame(window, layout.report, &.{});
            }

            // show source
            const buffer = self.error_source_editor.buffer;
            self.error_source_editor.deinit();
            buffer.deinit();
            const error_source_buffer = Buffer.initFromAbsoluteFilename(self.app, .Preview, error_location.path);
            self.error_source_editor = Editor.init(self.app, error_source_buffer, false, false);
            {
                const cursor = self.error_source_editor.getMainCursor();
                self.error_source_editor.goRealLineCol(cursor, error_location.line - 1, error_location.col - 1);
                self.error_source_editor.setCenterAtPos(cursor.head.pos);
            }
            self.error_source_editor.frame(window, layout.preview, &.{});
        }
    }
};
