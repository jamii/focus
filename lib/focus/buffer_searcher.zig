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
    target_buffer_id: Id,
    target_editor_id: Id,
    input_buffer_id: Id,
    input_editor_id: Id,
    selector: Selector,

    pub fn init(app: *App, target_buffer_id: Id, target_editor_id: Id, init_search: []const u8) Id {
        const input_buffer_id = Buffer.initEmpty(app);
        const input_editor_id = Editor.init(app, input_buffer_id);
        var input_editor = app.getThing(input_editor_id).Editor;
        input_editor.insert(input_editor.getMainCursor(), init_search);

        const selector = Selector.init(app);

        return app.putThing(BufferSearcher{
            .app = app,
            .target_buffer_id = target_buffer_id,
            .target_editor_id = target_editor_id,
            .input_buffer_id = input_buffer_id,
            .input_editor_id = input_editor_id,
            .selector = selector,
        });
    }

    pub fn deinit(self: *BufferSearcher) void {
    }

    pub fn frame(self: *BufferSearcher, window: *Window, rect: Rect, events: []const c.SDL_Event) void {
        var target_buffer = self.app.getThing(self.target_buffer_id).Buffer;
        var target_editor = self.app.getThing(self.target_editor_id).Editor;
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
            const max_line_string = format(self.app.frame_allocator, "{}", .{target_buffer.countLines()});
            target_editor.collapseCursors();
            target_editor.setMark();
            if (filter.len > 0) {
                var pos: usize = 0;
                var i: usize = 0;
                while (target_buffer.searchForwards(pos, filter)) |found_pos| {
                    const start = target_buffer.getLineStart(found_pos);
                    const end = target_buffer.getLineEnd(found_pos + filter.len);
                    const selection = target_buffer.dupe(self.app.frame_allocator, start, end);
                    assert(selection[0] != '\n' and selection[selection.len-1] != '\n');

                    var result = ArrayList(u8).init(self.app.frame_allocator);
                    const line = target_buffer.getLineColForPos(found_pos)[0];
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

        // split rect
        var all_rect = rect;
        const target_rect = all_rect.splitTop(@divTrunc(rect.h, 2), 0);
        const border1_rect = all_rect.splitTop(@divTrunc(self.app.atlas.char_height, 8), 0);
        const input_rect = all_rect.splitBottom(self.app.atlas.char_height, 0);
        const border2_rect = all_rect.splitBottom(@divTrunc(self.app.atlas.char_height, 8), 0);
        const selector_rect = all_rect;
        window.queueRect(border1_rect, style.text_color);
        window.queueRect(border2_rect, style.text_color);

        // run selector frame
        const action = self.selector.frame(window, selector_rect, selector_events.toOwnedSlice(), results.items);
        switch (action) {
            .None, .SelectRaw => {},
            .SelectOne, .SelectAll => window.popView(),
        }

        // preview
        // TODO centre view on main cursor
        target_editor.collapseCursors();
        switch (action) {
            .None, .SelectRaw, .SelectOne => {
                if (result_pos.items.len != 0) {
                    const pos = result_pos.items[self.selector.selected];
                    var cursor = target_editor.getMainCursor();
                    target_editor.goPos(cursor, pos);
                    target_editor.updatePos(&cursor.tail, pos + filter.len);
                }
            },
            .SelectAll => {
                for (result_pos.items) |pos, i| {
                    var cursor = if (i == 0) target_editor.getMainCursor() else target_editor.newCursor();
                    target_editor.goPos(cursor, pos);
                    target_editor.updatePos(&cursor.tail, pos + filter.len);
                }
            }
        }

        // run other editor frames
        target_editor.frame(window, target_rect, &[0]c.SDL_Event{});
        input_editor.frame(window, input_rect, input_events.toOwnedSlice());
    }
};
