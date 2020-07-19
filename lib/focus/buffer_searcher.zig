const focus = @import("../focus.zig");
usingnamespace focus.common;
const meta = focus.meta;
const App = focus.App;
const Id = focus.Id;
const Buffer = focus.Buffer;
const Editor = focus.Editor;
const Selector = focus.Selector;
const Window = focus.Window;
const style = focus.style;

pub const BufferSearcher = struct {
    app: *App,
    preview_buffer_id: Id,
    preview_editor_id: Id,
    input_buffer_id: Id,
    input_editor_id: Id,
    selector: Selector,

    pub fn init(app: *App, preview_buffer_id: Id, preview_editor_id: Id, init_search: []const u8) Id {
        const input_buffer_id = Buffer.initEmpty(app);
        const input_editor_id = Editor.init(app, input_buffer_id);
        var input_editor = app.getThing(input_editor_id).Editor;
        input_editor.insert(input_editor.getMainCursor(), init_search);

        const selector = Selector.init(app);

        return app.putThing(BufferSearcher{
            .app = app,
            .preview_buffer_id = preview_buffer_id,
            .preview_editor_id = preview_editor_id,
            .input_buffer_id = input_buffer_id,
            .input_editor_id = input_editor_id,
            .selector = selector,
        });
    }

    pub fn deinit(self: *BufferSearcher) void {
    }

    pub fn frame(self: *BufferSearcher, window: *Window, rect: Rect, events: []const c.SDL_Event) void {
        var preview_buffer = self.app.getThing(self.preview_buffer_id).Buffer;
        var preview_editor = self.app.getThing(self.preview_editor_id).Editor;
        var input_buffer = self.app.getThing(self.input_buffer_id).Buffer;
        var input_editor = self.app.getThing(self.input_editor_id).Editor;

        // handle events
        var input_events = ArrayList(c.SDL_Event).init(self.app.frame_allocator);
        var selector_events = ArrayList(c.SDL_Event).init(self.app.frame_allocator);
        for (events) |event| {
            switch (event.type) {
                c.SDL_KEYDOWN => {
                    const sym = event.key.keysym;
                    if (sym.mod == c.KMOD_LCTRL or sym.mod == c.KMOD_RCTRL) {
                        switch (sym.sym) {
                            'q' => window.popView(),
                            'k', 'i', c.SDLK_RETURN => selector_events.append(event) catch oom(),
                            else => input_events.append(event) catch oom(),
                        }
                    } else if (sym.mod == c.KMOD_LALT or sym.mod == c.KMOD_RALT) {
                        switch (sym.sym) {
                            'k', 'i', c.SDLK_RETURN => selector_events.append(event) catch oom(),
                            else => input_events.append(event) catch oom(),
                        }
                    } else if (sym.mod == 0) {
                        switch (sym.sym) {
                            c.SDLK_RETURN => selector_events.append(event) catch oom(),
                            else => input_events.append(event) catch oom(),
                        }
                    } else {
                        input_events.append(event) catch oom();
                    }
                },
                c.SDL_MOUSEWHEEL => {},
                else => input_events.append(event) catch oom(),
            }
        }

        // split rect
        var all_rect = rect;
        const preview_rect = all_rect.splitTop(@divTrunc(rect.h, 2), 0);
        const border1_rect = all_rect.splitTop(@divTrunc(self.app.atlas.char_height, 8), 0);
        const input_rect = all_rect.splitBottom(self.app.atlas.char_height, 0);
        const border2_rect = all_rect.splitBottom(@divTrunc(self.app.atlas.char_height, 8), 0);
        const selector_rect = all_rect;
        window.queueRect(border1_rect, style.text_color);
        window.queueRect(border2_rect, style.text_color);

        // run input frame
        input_editor.frame(window, input_rect, input_events.toOwnedSlice());

        // remove any sneaky newlines
        {
            var pos: usize = 0;
            while (input_buffer.searchForwards(pos, "\n")) |new_pos| {
                pos = new_pos;
                input_editor.delete(pos, pos + 1);
            }
        }

        // search buffer
        const filter = input_buffer.bytes.items;
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
                    assert(selection[0] != '\n' and selection[selection.len-1] != '\n');

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
        const action = self.selector.frame(window, selector_rect, selector_events.toOwnedSlice(), results.items);
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
            }
        }

        // run preview frame
        preview_editor.frame(window, preview_rect, &[0]c.SDL_Event{});
    }
};
