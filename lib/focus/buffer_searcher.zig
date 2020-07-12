const focus = @import("../focus.zig");
usingnamespace focus.common;
const meta = focus.meta;
const App = focus.App;
const Id = focus.Id;
const Buffer = focus.Buffer;
const Editor = focus.Editor;
const Window = focus.Window;
const style = focus.style;

// TODO arena allocator for lifespan of opener

// TODO always have a selection, open first match on enter (reject if no match), open raw text on ctrl-enter

pub const BufferSearcher = struct {
    app: *App,
    target_buffer_id: Id,
    target_editor_id: Id,
    input_buffer_id: Id,
    input_editor_id: Id,
    completions_buffer_id: Id,
    completions_editor_id: Id,
    selected: usize, // 0 for nothing selected, i-1 for line i

    pub fn init(app: *App, target_buffer_id: Id, target_editor_id: Id, init_filter: []const u8) !Id {
        // TODO don't directly mutate buffer - messes up multiple cursors - go via editor instead
        const input_buffer_id = try Buffer.initEmpty(app);
        const input_editor_id = try Editor.init(app, input_buffer_id);
        const completions_buffer_id = try Buffer.initEmpty(app);
        const completions_editor_id = try Editor.init(app, completions_buffer_id);

        // set initial filter
        try app.getThing(input_buffer_id).Buffer.insert(0, init_filter);

        // start cursor at end
        var input_editor = app.getThing(input_editor_id).Editor;
        input_editor.goBufferEnd(input_editor.getMainCursor());

        // TODO set default selection after point at which we started the search from?

        return app.putThing(BufferSearcher{
            .app = app,
            .target_buffer_id = target_buffer_id,
            .target_editor_id = target_editor_id,
            .input_buffer_id = input_buffer_id,
            .input_editor_id = input_editor_id,
            .completions_buffer_id = completions_buffer_id,
            .completions_editor_id = completions_editor_id,
            .selected = 0,
        });
    }

    pub fn deinit(self: *BufferSearcher) void {
        self.completions.deinit();
    }

    pub fn frame(self: *BufferSearcher, window: *Window, rect: Rect, events: []const c.SDL_Event) !void {
        var target_buffer = self.app.getThing(self.target_buffer_id).Buffer;
        var target_editor = self.app.getThing(self.target_editor_id).Editor;
        var input_buffer = self.app.getThing(self.input_buffer_id).Buffer;
        var input_editor = self.app.getThing(self.input_editor_id).Editor;
        var completions_buffer = self.app.getThing(self.completions_buffer_id).Buffer;
        var completions_editor = self.app.getThing(self.completions_editor_id).Editor;

        const Action = enum {
            None,
            SelectOne,
            SelectAll,
        };
        var action: Action = .None;

        // handle events
        var input_editor_events = ArrayList(c.SDL_Event).init(self.app.allocator);
        defer input_editor_events.deinit();
        for (events) |event| {
            var delegate = false;
            switch (event.type) {
                c.SDL_KEYDOWN => {
                    const sym = event.key.keysym;
                    if (sym.mod == c.KMOD_LCTRL or sym.mod == c.KMOD_RCTRL) {
                        switch (sym.sym) {
                            'q' => window.popView(),
                            'k' => self.selected += 1,
                            'i' => if (self.selected != 0) {
                                self.selected -= 1;
                            },
                            else => delegate = true,
                        }
                    } else if (sym.mod == c.KMOD_LALT or sym.mod == c.KMOD_RALT) {
                        switch (sym.sym) {
                            'k' => self.selected = completions_buffer.countLines() - 1,
                            'i' => self.selected = 1,
                            c.SDLK_RETURN => {
                                action = .SelectAll;
                            },
                            else => delegate = true,
                        }
                    } else if (sym.mod == 0) {
                        switch (sym.sym) {
                            c.SDLK_RETURN => {
                                action = .SelectOne;
                            },
                            else => delegate = true,
                        }
                    } else {
                        delegate = true;
                    }
                },
                c.SDL_MOUSEWHEEL => {},
                else => delegate = true,
            }
            // delegate other events to input editor
            if (delegate) try input_editor_events.append(event);
        }

        // remove any sneaky newlines
        {
            var pos: usize = 0;
            while (input_buffer.searchForwards(pos, "\n")) |new_pos| {
                pos = new_pos;
                input_editor.delete(pos, pos + 1);
            }
        }

        // filter completions
        {
            const max_line_string = try format(self.app.allocator, "{}", .{target_buffer.countLines()});
            defer self.app.allocator.free(max_line_string);
            const filter = input_buffer.bytes.items;
            var pos: usize = 0;
            var i: usize = 0;
            completions_buffer.bytes.shrink(0);
            try target_editor.collapseCursors();
            target_editor.setMark();
            if (action != .None) {
                window.popView();
            }
            if (filter.len > 0) {
                while (target_buffer.searchForwards(pos, filter)) |found_pos| {
                    const start = target_buffer.getLineStart(found_pos);
                    const end = target_buffer.getLineEnd(found_pos + filter.len);
                    const selection = try target_buffer.dupe(self.app.allocator, start, end);
                    defer self.app.allocator.free(selection);
                    assert(selection[0] != '\n' and selection[selection.len-1] != '\n');

                    switch (action) {
                        .None, .SelectOne => {
                            if (i + 1 == self.selected) {
                                var cursor = target_editor.getMainCursor();
                                target_editor.goPos(cursor, found_pos);
                                target_editor.updatePos(&cursor.tail, found_pos + filter.len);
                            }
                        },
                        .SelectAll => {
                            var cursor = if (i == 0) target_editor.getMainCursor() else try target_editor.newCursor();
                            target_editor.goPos(cursor, found_pos);
                            target_editor.updatePos(&cursor.tail, found_pos + filter.len);
                        }
                    }

                    // TODO highlight found area
                    const line = target_buffer.getLineColForPos(found_pos)[0];
                    const line_string = try format(self.app.allocator, "{}", .{line});
                    defer self.app.allocator.free(line_string);
                    try completions_buffer.bytes.appendSlice(line_string);
                    try completions_buffer.bytes.appendNTimes(' ', max_line_string.len - line_string.len + 1);
                    try completions_buffer.bytes.appendSlice(selection);
                    try completions_buffer.bytes.append('\n');
                    pos = found_pos + filter.len;
                    i += 1;
                }
            }
        }

        // set selection
        self.selected = min(self.selected, completions_buffer.countLines());
        var cursor = completions_editor.getMainCursor();
        if (self.selected != 0) {
            completions_editor.goPos(cursor, completions_buffer.getPosForLineCol(self.selected - 1, 0));
            completions_editor.setMark();
            completions_editor.goLineEnd(cursor);
        } else {
            completions_editor.clearMark();
            completions_editor.goBufferStart(cursor);
        }

        // run editor frames
        var all_rect = rect;
        const target_rect = all_rect.splitTop(@divTrunc(rect.h, 2), 0);
        const border1_rect = all_rect.splitTop(@divTrunc(self.app.atlas.char_height, 8), 0);
        const input_rect = all_rect.splitBottom(self.app.atlas.char_height, 0);
        const border2_rect = all_rect.splitBottom(@divTrunc(self.app.atlas.char_height, 8), 0);
        const completions_rect = all_rect;
        try target_editor.frame(window, target_rect, &[0]c.SDL_Event{});
        try window.queueRect(border1_rect, style.text_color);
        try completions_editor.frame(window, completions_rect, &[0]c.SDL_Event{});
        try window.queueRect(border2_rect, style.text_color);
        try input_editor.frame(window, input_rect, input_editor_events.items);
    }
};
