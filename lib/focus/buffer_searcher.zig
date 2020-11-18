const focus = @import("../focus.zig");
usingnamespace focus.common;
const meta = focus.meta;
const App = focus.App;
const Buffer = focus.Buffer;
const Editor = focus.Editor;
const SingleLineEditor = focus.SingleLineEditor;
const Selector = focus.Selector;
const Window = focus.Window;
const style = focus.style;

pub const BufferSearcher = struct {
    app: *App,
    target_editor: *Editor,
    preview_editor: *Editor,
    input: SingleLineEditor,
    selector: Selector,

    pub fn init(app: *App, target_editor: *Editor, init_search: []const u8) BufferSearcher {
        const preview_editor = Editor.init(app, target_editor.buffer, false);
        preview_editor.getMainCursor().* = target_editor.getMainCursor().*;
        const input = SingleLineEditor.init(app, init_search);
        const selector = Selector.init(app);
        return BufferSearcher{
            .app = app,
            .target_editor = target_editor,
            .preview_editor = preview_editor,
            .input = input,
            .selector = selector,
        };
    }

    pub fn deinit(self: *BufferSearcher) void {
        self.selector.deinit();
        self.input.deinit();
        self.preview_editor.deinit();
        // dont own target_editor
    }

    pub fn frame(self: *BufferSearcher, window: *Window, rect: Rect, events: []const c.SDL_Event) void {
        const layout = window.layoutSearcher(rect);

        // run input frame
        self.input.frame(window, layout.input, events);

        // search buffer
        // TODO default to first result after target
        const filter = self.input.getText();
        var results = ArrayList([]const u8).init(self.app.frame_allocator);
        var result_pos = ArrayList(usize).init(self.app.frame_allocator);
        {
            const max_line_string = format(self.app.frame_allocator, "{}", .{self.preview_editor.buffer.countLines()});
            if (filter.len > 0) {
                var pos: usize = 0;
                var i: usize = 0;
                while (self.preview_editor.buffer.searchForwards(pos, filter)) |found_pos| {
                    const start = self.preview_editor.buffer.getLineStart(found_pos);
                    const end = self.preview_editor.buffer.getLineEnd(found_pos + filter.len);
                    const selection = self.preview_editor.buffer.dupe(self.app.frame_allocator, start, end);
                    assert(selection[0] != '\n' and selection[selection.len - 1] != '\n');

                    var result = ArrayList(u8).init(self.app.frame_allocator);
                    const line = self.preview_editor.buffer.getLineColForPos(found_pos)[0];
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
            .SelectOne, .SelectAll => {
                self.updateEditor(self.target_editor, action, result_pos.items, filter);
                self.target_editor.top_pixel = self.preview_editor.top_pixel;
                window.popView();
            },
        }

        // update preview
        self.updateEditor(self.preview_editor, action, result_pos.items, filter);

        // run preview frame
        self.preview_editor.frame(window, layout.preview, &[0]c.SDL_Event{});
    }

    fn updateEditor(self: *BufferSearcher, editor: *Editor, action: Selector.Action, result_pos: []usize, filter: []const u8) void {
        // TODO centre view on main cursor
        editor.collapseCursors();
        editor.setMark();
        switch (action) {
            .None, .SelectRaw, .SelectOne => {
                if (result_pos.len != 0) {
                    const pos = result_pos[self.selector.selected];
                    var cursor = editor.getMainCursor();
                    editor.goPos(cursor, pos + filter.len);
                    editor.updatePos(&cursor.tail, pos);
                }
            },
            .SelectAll => {
                for (result_pos) |pos, i| {
                    var cursor = if (i == 0) editor.getMainCursor() else editor.newCursor();
                    editor.goPos(cursor, pos + filter.len);
                    editor.updatePos(&cursor.tail, pos);
                }
            },
        }
    }
};
