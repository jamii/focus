const focus = @import("../focus.zig");
usingnamespace focus.common;
const meta = focus.meta;
const App = focus.App;
const Id = focus.Id;
const Buffer = focus.Buffer;
const Editor = focus.Editor;
const SingleLineEditor = focus.SingleLineEditor;
const Selector = focus.Selector;
const Window = focus.Window;
const style = focus.style;

pub const BufferSearcher = struct {
    app: *App,
    preview_buffer_id: Id,
    preview_editor_id: Id,
    input: SingleLineEditor,
    selector: Selector,

    pub fn init(app: *App, preview_buffer_id: Id, preview_editor_id: Id, init_search: []const u8) Id {
        const input = SingleLineEditor.init(app, init_search);
        const selector = Selector.init(app);
        return app.putThing(BufferSearcher{
            .app = app,
            .preview_buffer_id = preview_buffer_id,
            .preview_editor_id = preview_editor_id,
            .input = input,
            .selector = selector,
        });
    }

    pub fn deinit(self: *BufferSearcher) void {}

    pub fn frame(self: *BufferSearcher, window: *Window, rect: Rect, events: []const c.SDL_Event) void {
        var preview_buffer = self.app.getThing(self.preview_buffer_id).Buffer;
        var preview_editor = self.app.getThing(self.preview_editor_id).Editor;

        const layout = window.layoutSearcher(rect);

        // run input frame
        self.input.frame(window, layout.input, events);

        // search buffer
        const filter = self.input.getText();
        var results = ArrayList([]const u8).init(self.app.frame_allocator);
        var result_pos = ArrayList(usize).init(self.app.frame_allocator);
        {
            const max_line_string = format(self.app.frame_allocator, "{}", .{preview_buffer.countLines()});
            preview_editor.collapseCursors();
            preview_editor.setMark();
            if (filter.len > 0) {
                var pos: usize = 0;
                var i: usize = 0;
                while (preview_buffer.searchForwards(pos, filter)) |found_pos| {
                    const start = preview_buffer.getLineStart(found_pos);
                    const end = preview_buffer.getLineEnd(found_pos + filter.len);
                    const selection = preview_buffer.dupe(self.app.frame_allocator, start, end);
                    assert(selection[0] != '\n' and selection[selection.len - 1] != '\n');

                    var result = ArrayList(u8).init(self.app.frame_allocator);
                    const line = preview_buffer.getLineColForPos(found_pos)[0];
                    const line_string = format(self.app.frame_allocator, "{}", .{line});
                    result.appendSlice(line_string) catch oom();
                    result.appendNTimes(' ', max_line_string.len - line_string.len + 1) catch oom();
                    result.appendSlice(selection) catch oom();

                    results.append(result.toOwnedSlice()) catch oom();
                    result_pos.append(found_pos) catch oom();

                    pos = found_pos + filter.len;
                    i += 1;
                }
            }
        }

        // run selector frame
        const action = self.selector.frame(window, layout.selector, events, results.items);
        switch (action) {
            .None, .SelectRaw => {},
            .SelectOne, .SelectAll => window.popView(),
        }

        // update preview
        // TODO centre view on main cursor
        preview_editor.collapseCursors();
        switch (action) {
            .None, .SelectRaw, .SelectOne => {
                if (result_pos.items.len != 0) {
                    const pos = result_pos.items[self.selector.selected];
                    var cursor = preview_editor.getMainCursor();
                    preview_editor.goPos(cursor, pos);
                    preview_editor.updatePos(&cursor.tail, pos + filter.len);
                }
            },
            .SelectAll => {
                for (result_pos.items) |pos, i| {
                    var cursor = if (i == 0) preview_editor.getMainCursor() else preview_editor.newCursor();
                    preview_editor.goPos(cursor, pos);
                    preview_editor.updatePos(&cursor.tail, pos + filter.len);
                }
            },
        }

        // run preview frame
        preview_editor.frame(window, layout.preview, &[0]c.SDL_Event{});
    }
};
