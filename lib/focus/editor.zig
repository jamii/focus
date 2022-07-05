const std = @import("std");
const glfw = @import("glfw");
const focus = @import("../focus.zig");
const u = focus.util;
const c = focus.util.c;
const App = focus.App;
const Buffer = focus.Buffer;
const LineWrappedBuffer = focus.LineWrappedBuffer;
const BufferSearcher = focus.BufferSearcher;
//const ImpRepl = focus.ImpRepl;
const Window = focus.Window;
const style = focus.style;
const mach_compat = focus.mach_compat;

pub const Point = struct {
    // what char we're at
    // 0 <= pos <= buffer.getBufferEnd()
    pos: usize,

    // what column we 'want' to be at
    // should only be updated by left/right movement
    // 0 <= col
    col: usize,
};

pub const Cursor = struct {
    // the actual cursor
    head: Point,
    // the other end of the selection, if editor.marked
    tail: Point,
};

pub const Dragging = enum {
    NotDragging,
    Dragging,
    CtrlDragging,
};

pub const Options = struct {
    show_status_bar: bool = true,
    show_completer: bool = true,
};

pub const Editor = struct {
    app: *App,
    buffer: *Buffer,
    line_wrapped_buffer: LineWrappedBuffer,
    // cursors.len > 0
    cursors: u.ArrayList(Cursor),
    prev_main_cursor_head_pos: usize,
    marked: bool,
    dragging: Dragging,
    // which pixel of the buffer is at the top of the viewport
    top_pixel: isize,
    last_text_rect_h: u.Coord,
    wanted_center_pos: ?usize,
    last_event_ms: i64,
    options: Options,
    completer_o: ?Completer,

    const Completer = struct {
        prefix: []const u8,
        prefix_pos: usize,
        next_completion_ix: usize,
        last_pos: usize,
    };

    const scroll_amount = 32;
    const max_completions_shown = 10;

    pub fn init(app: *App, buffer: *Buffer, options: Options) *Editor {
        const line_wrapped_buffer = LineWrappedBuffer.init(app, buffer, std.math.maxInt(usize));
        var cursors = u.ArrayList(Cursor).init(app.allocator);
        cursors.append(.{
            .head = .{ .pos = 0, .col = 0 },
            .tail = .{ .pos = 0, .col = 0 },
        }) catch u.oom();
        const completer_o = if (options.show_completer)
            Completer{
                .prefix = "",
                .prefix_pos = 0,
                .next_completion_ix = 0,
                .last_pos = 0,
            }
        else
            null;
        var self = app.allocator.create(Editor) catch u.oom();
        self.* = Editor{
            .app = app,
            .buffer = buffer,
            .line_wrapped_buffer = line_wrapped_buffer,
            .cursors = cursors,
            .prev_main_cursor_head_pos = 0,
            .marked = false,
            .dragging = .NotDragging,
            .top_pixel = 0,
            .last_text_rect_h = 0,
            .wanted_center_pos = null,
            .last_event_ms = app.frame_time_ms,
            .options = options,
            .completer_o = completer_o,
        };
        buffer.registerEditor(self);

        // restore last cursor head / center pos
        var cursor = self.getMainCursor();
        self.goPos(cursor, u.min(self.buffer.getBufferEnd(), self.buffer.last_cursor_head));
        self.wanted_center_pos = self.buffer.last_center_pos;

        return self;
    }

    pub fn deinit(self: *Editor) void {
        if (self.completer_o) |completer| self.app.allocator.free(completer.prefix);
        self.buffer.deregisterEditor(self);
        self.cursors.deinit();
        self.line_wrapped_buffer.deinit();
        self.app.allocator.destroy(self);
    }

    pub fn frame(self: *Editor, window: *Window, frame_rect: u.Rect, events: []const mach_compat.Event) void {
        var text_rect = frame_rect;
        const status_rect = if (self.options.show_status_bar) text_rect.splitBottom(self.app.atlas.char_height, 0) else null;
        const left_gutter_rect = text_rect.splitLeft(self.app.atlas.char_width, 0);
        const right_gutter_rect = text_rect.splitRight(self.app.atlas.char_width, 0);

        // if window width has changed, need to update line wrapping and keep viewport at same pos
        const max_chars_per_line = @intCast(usize, @divTrunc(text_rect.w, self.app.atlas.char_width));
        if (self.line_wrapped_buffer.max_chars_per_line != max_chars_per_line) {
            const prev_center_pos = self.getCenterPos(self.last_text_rect_h);
            self.line_wrapped_buffer.max_chars_per_line = max_chars_per_line;
            self.line_wrapped_buffer.update();
            self.scrollPosToCenter(text_rect, prev_center_pos);
        }

        // if someone asked us to scroll a pos to center, do so
        if (self.wanted_center_pos) |wanted_center_pos| {
            self.scrollPosToCenter(text_rect, wanted_center_pos);
            self.wanted_center_pos = null;
        }

        // maybe start a new undo group
        // TODO should this be buffer.last_event_ms? or even just internal to buffer?
        if (self.app.frame_time_ms - self.last_event_ms > 500) {
            self.buffer.newUndoGroup();
        }

        // handle events
        var completer_event: enum {
            None,
            Down,
            Up,
        } = .None;
        for (events) |event| {
            self.last_event_ms = self.app.frame_time_ms;
            switch (event) {
                .key_press, .key_repeat => |key_event| {
                    if (key_event.mods.control and !key_event.mods.shift) {
                        switch (key_event.key) {
                            .space => self.toggleMark(),
                            .c => {
                                for (self.cursors.items) |*cursor| self.copy(cursor);
                                self.clearMark();
                            },
                            .x => {
                                for (self.cursors.items) |*cursor| self.cut(cursor);
                                self.clearMark();
                            },
                            .v => {
                                for (self.cursors.items) |*cursor| self.paste(cursor);
                                self.clearMark();
                            },
                            .j => for (self.cursors.items) |*cursor| self.goLeft(cursor),
                            .l => for (self.cursors.items) |*cursor| self.goRight(cursor),
                            .k => for (self.cursors.items) |*cursor| self.goWrappedDown(cursor),
                            .i => for (self.cursors.items) |*cursor| self.goWrappedUp(cursor),
                            .q => {
                                self.collapseCursors();
                                self.clearMark();
                            },
                            .d => self.addNextMatch(),
                            .s => self.save(.User),
                            .f => {
                                const buffer_searcher = BufferSearcher.init(self.app, self);
                                window.pushView(buffer_searcher);
                            },
                            .z => self.undo(text_rect),
                            .slash => for (self.cursors.items) |*cursor| self.modifyComment(cursor, .Insert),
                            .tab => for (self.cursors.items) |*cursor| self.indent(cursor),
                            .one => self.buffer.language.toggleMode(),
                            .zero => self.goMatchingParen(),
                            .backspace => self.deleteToken(),
                            else => {},
                        }
                    } else if (key_event.mods.control and key_event.mods.shift) {
                        switch (key_event.key) {
                            .z => self.redo(text_rect),
                            .d => self.removeLastMatch(),
                            else => {},
                        }
                    } else if (key_event.mods.alt) {
                        switch (key_event.key) {
                            .space => for (self.cursors.items) |*cursor| self.swapHead(cursor),
                            .j => for (self.cursors.items) |*cursor| self.goRealLineStart(cursor),
                            .l => for (self.cursors.items) |*cursor| self.goRealLineEnd(cursor),
                            .k => {
                                for (self.cursors.items) |*cursor| self.goBufferEnd(cursor);
                                // hardcode because we want to scroll even if cursor didn't move
                                const num_lines = self.line_wrapped_buffer.countLines();
                                self.top_pixel = @intCast(u.Coord, if (num_lines == 0) 0 else num_lines - 1) * self.app.atlas.char_height;
                            },
                            .i => {
                                for (self.cursors.items) |*cursor| self.goBufferStart(cursor);
                                // hardcode because we want to scroll even if cursor didn't move
                                self.top_pixel = 0;
                            },
                            .slash => for (self.cursors.items) |*cursor| self.modifyComment(cursor, .Remove),
                            else => {},
                        }
                    } else if (key_event.mods.shift) {
                        switch (key_event.key) {
                            .tab => completer_event = .Up,
                            else => {},
                        }
                    } else {
                        switch (key_event.key) {
                            .backspace => {
                                for (self.cursors.items) |*cursor| self.deleteBackwards(cursor);
                                self.clearMark();
                            },
                            .delete => {
                                for (self.cursors.items) |*cursor| self.deleteForwards(cursor);
                                self.clearMark();
                            },
                            .enter => {
                                for (self.cursors.items) |*cursor| {
                                    self.insert(cursor, &[1]u8{'\n'});
                                    self.indent(cursor);
                                }
                                self.clearMark();
                            },
                            .tab => completer_event = .Down,
                            else => {},
                        }
                    }
                },
                .char_input => |char_input_event| {
                    var text = [4]u8{ 0, 0, 0, 0 };
                    const len = std.unicode.utf8Encode(char_input_event.codepoint, &text) catch |err|
                        u.panic("Error encoding codepoint {}: {}", .{ char_input_event.codepoint, err });
                    for (self.cursors.items) |*cursor| self.insert(cursor, text[0..len]);
                    self.clearMark();
                },
                .mouse_press => |mouse_press_event| {
                    if (mouse_press_event.button == .left) {
                        const mouse_x = @floatToInt(u.Coord, mouse_press_event.pos.xpos);
                        const mouse_y = @floatToInt(u.Coord, mouse_press_event.pos.ypos);
                        if (text_rect.contains(mouse_x, mouse_y)) {
                            const line = @divTrunc(self.top_pixel + (mouse_y - text_rect.y), self.app.atlas.char_height);
                            const col = @divTrunc(mouse_x - text_rect.x + @divTrunc(self.app.atlas.char_width, 2), self.app.atlas.char_width);
                            const pos = self.line_wrapped_buffer.getPosForLineCol(u.min(self.line_wrapped_buffer.countLines() - 1, @intCast(usize, line)), @intCast(usize, col));
                            if (mouse_press_event.mods.control) {
                                self.dragging = .CtrlDragging;
                                var cursor = self.newCursor();
                                self.updatePos(&cursor.head, pos);
                                self.updatePos(&cursor.tail, pos);
                            } else {
                                self.dragging = .Dragging;
                                self.collapseCursors();
                                var cursor = self.getMainCursor();
                                self.updatePos(&cursor.head, pos);
                                self.updatePos(&cursor.tail, pos);
                                self.clearMark();
                            }
                        }
                    }
                },
                .mouse_release => |mouse_release_event| {
                    if (mouse_release_event.button == .left) {
                        self.dragging = .NotDragging;
                    }
                },
                .mouse_scroll => |mouse_scroll_event| {
                    self.top_pixel -= scroll_amount * @floatToInt(i16, mouse_scroll_event.yoffset);
                },
                else => {},
            }
        }

        // handle mouse drag
        // (might be dragging outside window, so can't rely on mouse_motion events)
        if (self.dragging != .NotDragging) {
            // get mouse state
            const mouse_pos = window.glfw_window.getCursorPos() catch |err|
                u.panic("Error getting cursor pos: {}", .{err});
            const mouse_x = @floatToInt(u.Coord, mouse_pos.xpos);
            const mouse_y = @floatToInt(u.Coord, mouse_pos.ypos);

            // update selection of dragged cursor
            const line = @divTrunc(self.top_pixel + mouse_y - text_rect.y, self.app.atlas.char_height);
            const col = @divTrunc(mouse_x - text_rect.x + @divTrunc(self.app.atlas.char_width, 2), self.app.atlas.char_width);
            const pos = self.line_wrapped_buffer.getPosForLineCol(u.min(self.line_wrapped_buffer.countLines() - 1, @intCast(usize, u.max(line, 0))), @intCast(usize, u.max(col, 0)));
            if (self.dragging == .CtrlDragging) {
                var cursor = &self.cursors.items[self.cursors.items.len - 1];
                if (cursor.tail.pos != pos and !self.marked) {
                    self.setMark();
                }
                self.updatePos(&cursor.head, pos);
            } else {
                for (self.cursors.items) |*cursor| {
                    if (cursor.tail.pos != pos and !self.marked) {
                        self.setMark();
                    }
                    self.updatePos(&cursor.head, pos);
                }
            }

            // if dragging outside window, scroll
            if (mouse_y <= text_rect.y) self.top_pixel -= scroll_amount;
            if (mouse_y >= text_rect.y + text_rect.h) self.top_pixel += scroll_amount;
        }

        // if cursor moved
        if (self.getMainCursor().head.pos != self.prev_main_cursor_head_pos) {
            // scroll it into editor
            self.prev_main_cursor_head_pos = self.getMainCursor().head.pos;
            self.scrollPosIntoView(text_rect, self.getMainCursor().head.pos);
        }

        // update buffer last cursor_head/center_pos
        self.buffer.last_cursor_head = self.getMainCursor().head.pos;
        self.buffer.last_center_pos = self.getCenterPos(text_rect.h);

        // calculate visible range
        // ensure we don't scroll off the top or bottom of the buffer
        const max_pixels = u.max(0,
        // height of text
        @intCast(isize, self.line_wrapped_buffer.countLines()) * @intCast(isize, self.app.atlas.char_height)
        // - half a screen
        - @divTrunc(text_rect.h, 2)
        // - 1 pixel to ensure that center_pos is always within text if possible
        - 1);
        if (self.top_pixel < 0) self.top_pixel = 0;
        if (self.top_pixel > max_pixels) self.top_pixel = max_pixels;
        const num_visible_lines = @divTrunc(text_rect.h, self.app.atlas.char_height) + @rem(@rem(text_rect.h, self.app.atlas.char_height), 1); // round up
        const visible_start_line = @divTrunc(self.top_pixel, self.app.atlas.char_height); // round down
        const visible_end_line = visible_start_line + num_visible_lines;

        // draw background
        window.queueRect(frame_rect, style.background_color);

        // can't draw if the window is too narrow
        if (max_chars_per_line == 0) return;

        // draw matching paren
        {
            const pos = self.getMainCursor().head.pos;
            if (pos > 0) {
                if (self.buffer.language.matchParen(pos)) |matching_pos| {
                    for ([2]usize{ pos - 1, matching_pos - 1 }) |highlight_pos| {
                        const line_col = self.line_wrapped_buffer.getLineColForPos(highlight_pos);
                        if (visible_start_line <= line_col[0] and line_col[0] <= visible_end_line) {
                            const line_top_pixel = @intCast(u.Coord, line_col[0]) * self.app.atlas.char_height;
                            const y = text_rect.y + @intCast(u.Coord, line_top_pixel - self.top_pixel);
                            const x = text_rect.x + @intCast(u.Coord, line_col[1]) * self.app.atlas.char_width;
                            window.queueRect(
                                .{
                                    .x = x,
                                    .y = @intCast(u.Coord, y),
                                    .w = self.app.atlas.char_width,
                                    .h = self.app.atlas.char_height,
                                },
                                style.paren_match_color,
                            );
                        }
                    }
                }
            }
        }

        // draw cursors, selections, text
        var line_ix = @intCast(usize, u.max(visible_start_line, 0));
        const max_line_ix = u.min(@intCast(usize, u.max(visible_end_line + 1, 0)), self.line_wrapped_buffer.wrapped_line_ranges.items.len);
        const min_pos = self.line_wrapped_buffer.wrapped_line_ranges.items[line_ix][0];
        const max_pos = self.line_wrapped_buffer.wrapped_line_ranges.items[max_line_ix - 1][1];
        const highlighter_colors = self.buffer.language.highlight(self.app.frame_allocator, self.buffer.bytes.items, .{ min_pos, max_pos });
        while (line_ix < max_line_ix) : (line_ix += 1) {
            const line_range = self.line_wrapped_buffer.wrapped_line_ranges.items[line_ix];
            const line = self.buffer.bytes.items[line_range[0]..line_range[1]];

            const line_top_pixel = @intCast(u.Coord, line_ix) * self.app.atlas.char_height;
            const y = text_rect.y + @intCast(u.Coord, line_top_pixel - self.top_pixel);

            const Squiggly = struct {
                color: u.Color,
                range: [2]usize,
            };
            var squigglies = u.ArrayList(Squiggly).init(self.app.frame_allocator);
            for (squigglies.items) |squiggly| {
                if (squiggly.range[0] != 0) {
                    const highlight_start_pos = u.min(u.max(squiggly.range[0], line_range[0]), line_range[1]);
                    const highlight_end_pos = u.min(u.max(squiggly.range[1], line_range[0]), line_range[1]);
                    if ((highlight_start_pos < highlight_end_pos) or (squiggly.range[0] <= line_range[1] and squiggly.range[1] > line_range[1])) {
                        const x = text_rect.x + (@intCast(u.Coord, (highlight_start_pos - line_range[0])) * self.app.atlas.char_width);
                        const w = if (squiggly.range[1] > line_range[1])
                            text_rect.x + text_rect.w - x
                        else
                            @intCast(u.Coord, (highlight_end_pos - highlight_start_pos)) * self.app.atlas.char_width;
                        var dx: i32 = 0;
                        while (dx < w) : (dx += self.app.atlas.char_width) {
                            window.queueText(.{
                                .x = @intCast(u.Coord, x + dx),
                                .y = @intCast(u.Coord, y) + @divTrunc(self.app.atlas.char_height, 2),
                                .w = self.app.atlas.char_width,
                                .h = self.app.atlas.char_height,
                            }, squiggly.color, "~");
                        }
                    }
                }
            }

            for (self.cursors.items) |cursor| {
                // draw cursor
                if (cursor.head.pos >= line_range[0] and cursor.head.pos <= line_range[1]) {
                    // blink
                    if (@mod(@divTrunc(self.app.frame_time_ms - self.last_event_ms, 500), 2) == 0) {
                        const x = text_rect.x + (@intCast(u.Coord, (cursor.head.pos - line_range[0])) * self.app.atlas.char_width);
                        const w = @divTrunc(self.app.atlas.char_width, 8);
                        window.queueRect(
                            .{
                                .x = @intCast(u.Coord, x) - @divTrunc(w, 2),
                                .y = @intCast(u.Coord, y),
                                .w = w,
                                .h = self.app.atlas.char_height,
                            },
                            if (self.cursors.items.len > 1) style.multi_cursor_color else style.text_color,
                        );
                    }
                }

                // draw selection
                if (self.marked) {
                    const selection_start_pos = u.min(cursor.head.pos, cursor.tail.pos);
                    const selection_end_pos = u.max(cursor.head.pos, cursor.tail.pos);
                    const highlight_start_pos = u.min(u.max(selection_start_pos, line_range[0]), line_range[1]);
                    const highlight_end_pos = u.min(u.max(selection_end_pos, line_range[0]), line_range[1]);
                    if ((highlight_start_pos < highlight_end_pos) or (selection_start_pos <= line_range[1] and selection_end_pos > line_range[1])) {
                        const x = text_rect.x + (@intCast(u.Coord, (highlight_start_pos - line_range[0])) * self.app.atlas.char_width);
                        const w = if (selection_end_pos > line_range[1])
                            text_rect.x + text_rect.w - x
                        else
                            @intCast(u.Coord, (highlight_end_pos - highlight_start_pos)) * self.app.atlas.char_width;
                        window.queueRect(.{
                            .x = @intCast(u.Coord, x),
                            .y = @intCast(u.Coord, y),
                            .w = @intCast(u.Coord, w),
                            .h = self.app.atlas.char_height,
                        }, style.highlight_color);
                    }
                }
            }

            // draw text
            // TODO clip text at the top of the editor, not only the bottom
            for (line) |char, i| {
                const highlighter_color = highlighter_colors[line_range[0] + i - min_pos];
                window.queueText(
                    .{
                        .x = text_rect.x + (@intCast(u.Coord, i) * self.app.atlas.char_width),
                        .y = @intCast(u.Coord, y),
                        .w = text_rect.w,
                        .h = text_rect.y + text_rect.h - @intCast(u.Coord, y),
                    },
                    highlighter_color,
                    &[1]u8{char},
                );
            }

            // if wrapped, draw arrows
            if (line_ix > 0 and line_range[0] == self.line_wrapped_buffer.wrapped_line_ranges.items[line_ix - 1][1]) {
                window.queueText(
                    .{ .x = left_gutter_rect.x, .y = @intCast(u.Coord, y), .h = self.app.atlas.char_height, .w = self.app.atlas.char_width },
                    style.highlight_color,
                    "\\",
                );
                // TODO need a font that has these arrows
                //window.queueQuad(
                //    .{ .x = left_gutter_rect.x, .y = @intCast(u.Coord, y), .h = self.app.atlas.char_height, .w = self.app.atlas.char_width },
                //    self.app.atlas.down_right_arrow_rect,
                //    style.highlight_color,
                //);
            }
            if (line_ix < self.line_wrapped_buffer.countLines() - 1 and line_range[1] == self.line_wrapped_buffer.wrapped_line_ranges.items[line_ix + 1][0]) {
                window.queueText(
                    .{ .x = right_gutter_rect.x, .y = @intCast(u.Coord, y), .h = self.app.atlas.char_height, .w = self.app.atlas.char_width },
                    style.highlight_color,
                    "\\",
                );
                // TODO need a font that has these arrows
                //window.queueQuad(
                //    .{ .x = right_gutter_rect.x, .y = @intCast(u.Coord, y), .h = self.app.atlas.char_height, .w = self.app.atlas.char_width },
                //    self.app.atlas.right_down_arrow_rect,
                //    style.highlight_color,
                //);
            }
        }

        if (self.completer_o) |*completer| completer: {
            if (self.dragging != .NotDragging) break :completer;

            // if cursor moved, reset completer prefix
            if (self.getMainCursor().head.pos != completer.last_pos) {
                self.app.allocator.free(completer.prefix);
                completer.prefix = self.app.dupe(self.buffer.getCompletionPrefix(self.getMainCursor().head.pos));
                completer.prefix_pos = self.getMainCursor().head.pos - completer.prefix.len;
                completer.next_completion_ix = 0;
                completer.last_pos = self.getMainCursor().head.pos;
            }

            if (completer.prefix.len == 0) break :completer;

            // get completions
            const completions = self.app.getCompletions(std.meta.activeTag(self.buffer.language), completer.prefix);

            if (completions.len == 0) break :completer;

            // rotate completions
            std.mem.rotate([]const u8, completions, completer.next_completion_ix);

            // figure out cursor position
            const cursor_linecol = self.line_wrapped_buffer.getLineColForPos(self.getMainCursor().head.pos);
            const cursor_x = text_rect.x + @intCast(u.Coord, cursor_linecol[1]) * self.app.atlas.char_width;
            const cursor_y = text_rect.y - @rem(self.top_pixel + 1, self.app.atlas.char_height) + ((@intCast(u.Coord, cursor_linecol[0]) - visible_start_line) * self.app.atlas.char_height);

            // figure out x placement
            var max_completion_chars: usize = 0;
            for (completions) |completion| {
                max_completion_chars = u.max(max_completion_chars, completion.len);
            }
            const prefix_x = cursor_x - (@intCast(u.Coord, (self.getMainCursor().head.pos - completer.prefix_pos)) * self.app.atlas.char_width);
            const completer_x = u.max(text_rect.x, u.min(text_rect.x + u.max(0, text_rect.w - (@intCast(u.Coord, max_completion_chars) * self.app.atlas.char_width)), prefix_x));
            const completer_w = u.min(text_rect.x + text_rect.w - completer_x, @intCast(u.Coord, max_completion_chars) * self.app.atlas.char_width);

            // figure out y placement
            const completer_y = @intCast(u.Coord, cursor_y) + @intCast(u.Coord, self.app.atlas.char_height);
            const available_h = text_rect.y + text_rect.h - completer_y;
            const completions_shown = u.min(max_completions_shown, completions.len);
            const fractional_completer_h = u.min(@intCast(u.Coord, available_h), @intCast(u.Coord, completions_shown) * self.app.atlas.char_height);
            const completer_h = fractional_completer_h - @rem(fractional_completer_h, self.app.atlas.char_height);

            const completer_rect = u.Rect{
                .x = completer_x,
                .y = completer_y,
                .h = completer_h,
                .w = completer_w,
            };

            // draw completions
            window.queueRect(completer_rect, style.status_background_color);
            for (completions) |completion, i| {
                if (i > completions_shown) break;
                var rect = completer_rect;
                rect.y += @intCast(u.Coord, i) * self.app.atlas.char_height;
                window.queueText(rect, style.text_color, completion);
            }

            // handle replacement
            if (completer_event != .None) {
                for (self.cursors.items) |*cursor|
                    self.buffer.insertCompletion(cursor.head.pos, self.app.frame_allocator.dupe(u8, completions[0]) catch u.oom());
                // make sure we don't get confused about that cursor movement
                completer.last_pos = self.getMainCursor().head.pos;
            }

            // handle movement
            switch (completer_event) {
                .None => {},
                .Up => completer.next_completion_ix = @intCast(usize, @mod(@intCast(isize, completer.next_completion_ix) - 1, @intCast(isize, completions.len))),
                .Down => completer.next_completion_ix = (completer.next_completion_ix + 1) % completions.len,
            }
        }

        // draw scrollbar
        {
            const ratio = if (max_pixels == 0) 0 else @intToFloat(f64, self.top_pixel) / @intToFloat(f64, max_pixels);
            const y = text_rect.y + u.min(@floatToInt(u.Coord, @intToFloat(f64, text_rect.h) * ratio), text_rect.h - self.app.atlas.char_height);
            var left_scroll_rect = left_gutter_rect;
            left_scroll_rect.y = y;
            left_scroll_rect.h -= y;
            window.queueText(left_scroll_rect, style.highlight_color, ">");
            var right_scroll_rect = right_gutter_rect;
            right_scroll_rect.y = y;
            right_scroll_rect.h -= y;
            window.queueText(right_scroll_rect, style.highlight_color, "<");
        }

        // draw statusbar
        if (self.options.show_status_bar) {
            window.queueRect(status_rect.?, style.status_background_color);
            // use real line_col instead of wrapped
            const line_col = self.buffer.getLineColForPos(self.getMainCursor().head.pos);
            const filename = self.buffer.getFilename() orelse "";
            const status_text = u.format(self.app.frame_allocator, "{s} L{} C{}", .{ filename, line_col[0] + 1, line_col[1] + 1 });
            window.queueText(status_rect.?, style.text_color, status_text);
        }

        self.last_text_rect_h = text_rect.h;
    }

    pub fn updateCol(self: *Editor, point: *Point) void {
        point.col = point.pos - self.line_wrapped_buffer.getLineStart(point.*.pos);
    }

    pub fn updatePos(self: *Editor, point: *Point, pos: usize) void {
        point.pos = pos;
        self.updateCol(point);
    }

    pub fn goPos(self: *Editor, cursor: *Cursor, pos: usize) void {
        self.updatePos(&cursor.head, pos);
    }

    pub fn goRealCol(self: *Editor, cursor: *Cursor, col: usize) void {
        const line_start = self.buffer.getLineStart(cursor.head.pos);
        cursor.head.col = u.min(col, self.buffer.getLineEnd(cursor.head.pos) - line_start);
        cursor.head.pos = line_start + cursor.head.col;
    }

    pub fn goRealLine(self: *Editor, cursor: *Cursor, line: usize) void {
        cursor.head.pos = self.buffer.getPosForLine(line);
        // leave head.col intact
    }

    pub fn goRealLineCol(self: *Editor, cursor: *Cursor, line: usize, col: usize) void {
        self.goRealLine(cursor, line);
        self.goRealCol(cursor, col);
    }

    pub fn tryGoRealLine(self: *Editor, cursor: *Cursor, line: usize) void {
        const safe_line = u.min(line, self.buffer.countLines() - 1);
        self.goRealLine(cursor, safe_line);
    }

    pub fn goWrappedCol(self: *Editor, cursor: *Cursor, col: usize) void {
        const line_start = self.line_wrapped_buffer.getLineStart(cursor.head.pos);
        cursor.head.col = u.min(col, self.line_wrapped_buffer.getLineEnd(cursor.head.pos) - line_start);
        cursor.head.pos = line_start + cursor.head.col;
    }

    pub fn goWrappedLine(self: *Editor, cursor: *Cursor, line: usize) void {
        cursor.head.pos = self.line_wrapped_buffer.getPosForLine(line);
        // leave head.col intact
    }

    pub fn goWrappedLineCol(self: *Editor, cursor: *Cursor, line: usize, col: usize) void {
        self.goWrappedLine(cursor, line);
        self.goWrappedCol(cursor, col);
    }

    pub fn goLeft(self: *Editor, cursor: *Cursor) void {
        cursor.head.pos -= @as(usize, if (cursor.head.pos == 0) 0 else 1);
        self.updateCol(&cursor.head);
    }

    pub fn goRight(self: *Editor, cursor: *Cursor) void {
        cursor.head.pos += @as(usize, if (cursor.head.pos >= self.buffer.getBufferEnd()) 0 else 1);
        self.updateCol(&cursor.head);
    }

    pub fn goRealDown(self: *Editor, cursor: *Cursor) void {
        const line_col = self.buffer.getLineColForPos(cursor.head.pos);
        if (line_col[0] < self.buffer.countLines() - 1) {
            const col = cursor.head.col;
            self.goRealLineCol(cursor, line_col[0] + 1, col);
            cursor.head.col = col;
        }
    }

    pub fn goRealUp(self: *Editor, cursor: *Cursor) void {
        const line_col = self.buffer.getLineColForPos(cursor.head.pos);
        if (line_col[0] > 0) {
            const col = cursor.head.col;
            self.goRealLineCol(cursor, line_col[0] - 1, col);
            cursor.head.col = col;
        }
    }

    pub fn goWrappedDown(self: *Editor, cursor: *Cursor) void {
        const line_col = self.line_wrapped_buffer.getLineColForPos(cursor.head.pos);
        if (line_col[0] < self.line_wrapped_buffer.countLines() - 1) {
            const col = cursor.head.col;
            self.goWrappedLineCol(cursor, line_col[0] + 1, col);
            cursor.head.col = col;
        }
    }

    pub fn goWrappedUp(self: *Editor, cursor: *Cursor) void {
        const line_col = self.line_wrapped_buffer.getLineColForPos(cursor.head.pos);
        if (line_col[0] > 0) {
            const col = cursor.head.col;
            self.goWrappedLineCol(cursor, line_col[0] - 1, col);
            cursor.head.col = col;
        }
    }

    pub fn goRealLineStart(self: *Editor, cursor: *Cursor) void {
        cursor.head.pos = self.buffer.getLineStart(cursor.head.pos);
        cursor.head.col = 0;
    }

    pub fn goRealLineEnd(self: *Editor, cursor: *Cursor) void {
        cursor.head.pos = self.buffer.getLineEnd(cursor.head.pos);
        self.updateCol(&cursor.head);
    }

    pub fn goWrappedLineStart(self: *Editor, cursor: *Cursor) void {
        cursor.head.pos = self.line_wrapped_buffer.getLineStart(cursor.head.pos);
        cursor.head.col = 0;
    }

    pub fn goWrappedLineEnd(self: *Editor, cursor: *Cursor) void {
        cursor.head.pos = self.line_wrapped_buffer.getLineEnd(cursor.head.pos);
        self.updateCol(&cursor.head);
    }

    pub fn goBufferStart(self: *Editor, cursor: *Cursor) void {
        self.goPos(cursor, 0);
    }

    pub fn goBufferEnd(self: *Editor, cursor: *Cursor) void {
        self.goPos(cursor, self.buffer.getBufferEnd());
    }

    pub fn insert(self: *Editor, cursor: *Cursor, bytes: []const u8) void {
        self.deleteSelection(cursor);
        self.buffer.insert(cursor.head.pos, bytes);
        // buffer calls updateAfterInsert
    }

    pub fn updateAfterInsert(self: *Editor, start: usize, bytes: []const u8) void {
        self.line_wrapped_buffer.update();
        for (self.cursors.items) |*cursor| {
            for (&[2]*Point{ &cursor.head, &cursor.tail }) |point| {
                // TODO ptr compare is because we want paste to leave each cursor after its own insert
                // if (point.pos > insert_at or (point.pos == insert_at and @ptrToInt(other_cursor) >= @ptrToInt(cursor))) {
                if (point.pos >= start) {
                    point.pos += bytes.len;
                    self.updateCol(point);
                }
            }
        }
    }

    pub fn delete(self: *Editor, start: usize, end: usize) void {
        u.assert(start <= end);
        self.buffer.delete(start, end);
        // buffer calls updateAfterDelete
    }

    pub fn updateAfterDelete(self: *Editor, start: usize, end: usize) void {
        self.line_wrapped_buffer.update();
        for (self.cursors.items) |*cursor| {
            for (&[2]*Point{ &cursor.head, &cursor.tail }) |point| {
                if (point.pos >= start and point.pos < end) point.pos = start;
                if (point.pos >= end) point.pos -= (end - start);
                self.updateCol(point);
            }
        }
    }

    pub fn updateBeforeReplace(self: *Editor) [][2][2]usize {
        var line_cols = u.ArrayList([2][2]usize).init(self.app.frame_allocator);
        for (self.cursors.items) |cursor| {
            line_cols.append(.{
                self.buffer.getLineColForPos(cursor.head.pos),
                self.buffer.getLineColForPos(cursor.tail.pos),
            }) catch u.oom();
        }
        return line_cols.toOwnedSlice();
    }

    pub fn updateAfterReplace(self: *Editor, line_cols: [][2][2]usize) void {
        self.line_wrapped_buffer.update();
        for (self.cursors.items) |*cursor, i| {
            for (&[2]*Point{ &cursor.head, &cursor.tail }) |point, j| {
                const line_col = line_cols[i][j];
                const pos = self.buffer.getPosForLineCol(u.min(line_col[0], self.buffer.countLines() - 1), line_col[1]);
                self.updatePos(point, pos);
            }
        }
    }

    pub fn deleteSelection(self: *Editor, cursor: *Cursor) void {
        if (self.marked) {
            const range = self.getSelectionRange(cursor);
            self.delete(range[0], range[1]);
        }
    }

    pub fn deleteBackwards(self: *Editor, cursor: *Cursor) void {
        if (self.marked) {
            self.deleteSelection(cursor);
        } else if (cursor.head.pos > 0) {
            self.delete(cursor.head.pos - 1, cursor.head.pos);
        }
    }

    pub fn deleteForwards(self: *Editor, cursor: *Cursor) void {
        if (self.marked) {
            self.deleteSelection(cursor);
        } else if (cursor.head.pos < self.buffer.getBufferEnd()) {
            self.delete(cursor.head.pos, cursor.head.pos + 1);
        }
    }

    pub fn clearMark(self: *Editor) void {
        self.marked = false;
    }

    pub fn setMark(self: *Editor) void {
        self.marked = true;
        for (self.cursors.items) |*cursor| {
            cursor.tail = cursor.head;
        }
    }

    pub fn toggleMark(self: *Editor) void {
        if (self.marked) {
            self.clearMark();
        } else {
            self.setMark();
        }
    }

    pub fn swapHead(self: *Editor, cursor: *Cursor) void {
        if (self.marked) {
            std.mem.swap(Point, &cursor.head, &cursor.tail);
        }
    }

    pub fn getSelectionRange(self: *Editor, cursor: *Cursor) [2]usize {
        if (self.marked) {
            const selection_start_pos = u.min(cursor.head.pos, cursor.tail.pos);
            const selection_end_pos = u.max(cursor.head.pos, cursor.tail.pos);
            return [2]usize{ selection_start_pos, selection_end_pos };
        } else {
            return [2]usize{ cursor.head.pos, cursor.head.pos };
        }
    }

    pub fn dupeSelection(self: *Editor, allocator: u.Allocator, cursor: *Cursor) []const u8 {
        const range = self.getSelectionRange(cursor);
        return self.buffer.dupe(allocator, range[0], range[1]);
    }

    // TODO clipboard stack on app
    // TODO figure out nicer situation for multiple cursor copy/paste
    // could use wl-paste -w 'focus-copy'

    pub fn copy(self: *Editor, cursor: *Cursor) void {
        if (cursor.head.pos == cursor.tail.pos) return;
        const text = self.dupeSelection(self.app.frame_allocator, cursor);
        const textZ = self.app.frame_allocator.dupeZ(u8, text) catch u.oom();
        glfw.setClipboardString(textZ) catch |err|
            u.panic("Error while setting system clipboard: {}", .{err});
    }

    pub fn cut(self: *Editor, cursor: *Cursor) void {
        self.copy(cursor);
        self.deleteSelection(cursor);
    }

    pub fn paste(self: *Editor, cursor: *Cursor) void {
        const text = glfw.getClipboardString() catch |err|
            u.panic("Error while getting system clipboard: {}", .{err});
        // text is owned by glfw, don't need to free
        self.insert(cursor, std.mem.span(text));
    }

    // TODO rename to addCursor
    pub fn newCursor(self: *Editor) *Cursor {
        self.cursors.append(.{
            .head = .{ .pos = 0, .col = 0 },
            .tail = .{ .pos = 0, .col = 0 },
        }) catch u.oom();
        return &self.cursors.items[self.cursors.items.len - 1];
    }

    pub fn collapseCursors(self: *Editor) void {
        self.cursors.shrinkAndFree(1);
    }

    pub fn getMainCursor(self: *Editor) *Cursor {
        return &self.cursors.items[self.cursors.items.len - 1];
    }

    pub fn addNextMatch(self: *Editor) void {
        const main_cursor = self.getMainCursor();
        const selection = self.dupeSelection(self.app.frame_allocator, main_cursor);
        if (self.buffer.searchForwards(u.max(main_cursor.head.pos, main_cursor.tail.pos), selection)) |pos| {
            var cursor = Cursor{
                .head = .{ .pos = pos + selection.len, .col = 0 },
                .tail = .{ .pos = pos, .col = 0 },
            };
            if (main_cursor.head.pos < main_cursor.tail.pos)
                std.mem.swap(Point, &cursor.head, &cursor.tail);
            self.updateCol(&cursor.head);
            self.updateCol(&cursor.tail);
            self.cursors.append(cursor) catch u.oom();
        }
    }

    pub fn removeLastMatch(self: *Editor) void {
        if (self.cursors.items.len > 1) {
            _ = self.cursors.pop();
        }
    }

    pub fn indent(self: *Editor, cursor: *Cursor) void {
        // figure out how many lines we're going to indent _before_ we start changing them
        var num_lines: usize = 1;
        const range = self.getSelectionRange(cursor);
        {
            var pos = range[0];
            while (self.buffer.searchForwards(pos, "\n")) |new_pos| {
                if (new_pos >= range[1]) break;
                num_lines += 1;
                pos = new_pos + 1;
            }
        }

        // make a new cursor to peform the indents
        var edit_cursor = self.newCursor();
        std.mem.swap(Cursor, edit_cursor, &self.cursors.items[0]);
        edit_cursor = &self.cursors.items[0];
        defer {
            std.mem.swap(Cursor, edit_cursor, &self.cursors.items[self.cursors.items.len - 1]);
            _ = self.cursors.pop();
        }
        self.goPos(edit_cursor, range[0]);

        // for each line in selection
        while (num_lines > 0) : (num_lines -= 1) {

            // work out current indent
            self.goRealLineStart(edit_cursor);
            const this_line_start_pos = edit_cursor.head.pos;
            var this_indent: usize = 0;
            while (this_line_start_pos + this_indent < self.buffer.bytes.items.len and self.buffer.bytes.items[this_line_start_pos + this_indent] == ' ') {
                this_indent += 1;
            }

            const ideal_indent = self.buffer.language.getIdealIndent(this_line_start_pos);

            // adjust indent
            edit_cursor.head.pos = this_line_start_pos;
            if (this_indent > ideal_indent) {
                self.delete(this_line_start_pos, this_line_start_pos + this_indent - ideal_indent);
            }
            if (this_indent < ideal_indent) {
                var spaces = self.app.frame_allocator.alloc(u8, ideal_indent - this_indent) catch u.oom();
                std.mem.set(u8, spaces, ' ');
                // TODO this might delete the selection :S
                const old_marked = self.marked;
                self.marked = false;
                self.insert(edit_cursor, spaces);
                self.marked = old_marked;
            }

            // go to next line in selection
            self.goRealLineEnd(edit_cursor);
            self.goRight(edit_cursor);
        }
    }

    pub fn modifyComment(self: *Editor, cursor: *Cursor, action: enum { Insert, Remove }) void {
        // see if we know how to comment this language
        const comment_string = self.buffer.language.getCommentString() orelse return;

        // figure out how many lines we're going to comment _before_ we start changing them
        const range = self.getSelectionRange(cursor);
        const start_line = self.buffer.getLineColForPos(range[0])[0];
        const end_line = self.buffer.getLineColForPos(range[1])[0];

        switch (action) {
            .Insert => {
                // find minimum indent
                var minimum_indent: usize = std.math.maxInt(usize);
                {
                    var line: usize = start_line;
                    while (line <= end_line) : (line += 1) {
                        const start = self.buffer.getPosForLine(line);
                        const end = self.buffer.getLineEnd(start);

                        // find first non-whitespace char
                        for (self.buffer.bytes.items[start..end]) |byte, i| {
                            if (byte != ' ') {
                                minimum_indent = u.min(minimum_indent, i);
                                break;
                            }
                        }
                    }
                }

                // comment each line that has non-whitespace chars
                {
                    var line: usize = start_line;
                    while (line <= end_line) : (line += 1) {
                        const start = self.buffer.getPosForLine(line);
                        const end = self.buffer.getLineEnd(start);

                        // check for any non-whitespace characters
                        var any_non_whitespace = false;
                        for (self.buffer.bytes.items[start..end]) |byte| {
                            if (byte != ' ') {
                                any_non_whitespace = true;
                                break;
                            }
                        }

                        // insert comment
                        if (any_non_whitespace)
                            self.buffer.insert(start + minimum_indent, comment_string);
                    }
                }
            },
            .Remove => {
                var line: usize = start_line;
                while (line <= end_line) : (line += 1) {
                    const start = self.buffer.getPosForLine(line);
                    const end = self.buffer.getLineEnd(start);

                    // remove first comment
                    if (self.buffer.searchForwards(start, comment_string)) |pos|
                        if (pos <= end)
                            self.buffer.delete(pos, pos + comment_string.len);
                }
            },
        }
    }

    pub fn tryFormat(self: *Editor) void {
        if (self.buffer.language.format(self.buffer.bytes.items)) |new_source|
            self.buffer.replace(new_source);
    }

    pub fn save(self: *Editor, source: Buffer.SaveSource) void {
        if (self.buffer.modified_since_last_save) {
            self.tryFormat();
            self.buffer.save(source);
        }
    }

    pub fn undo(self: *Editor, text_rect: u.Rect) void {
        const pos_o = self.buffer.undo();
        if (pos_o) |pos| {
            self.collapseCursors();
            self.clearMark();
            var cursor = self.getMainCursor();
            self.goPos(cursor, pos);
            self.scrollPosToCenter(text_rect, pos);
            cursor.tail = cursor.head;
        }
    }

    pub fn redo(self: *Editor, text_rect: u.Rect) void {
        const pos_o = self.buffer.redo();
        if (pos_o) |pos| {
            self.collapseCursors();
            self.clearMark();
            var cursor = self.getMainCursor();
            self.goPos(cursor, pos);
            self.scrollPosToCenter(text_rect, pos);
            cursor.tail = cursor.head;
        }
    }

    pub fn getCenterPos(self: *Editor, text_rect_h: u.Coord) usize {
        const prev_center_wrapped_line = @intCast(usize, @divTrunc(self.top_pixel + @divTrunc(text_rect_h, 2), self.app.atlas.char_height));
        return self.line_wrapped_buffer.getPosForLine(u.min(self.line_wrapped_buffer.countLines() - 1, prev_center_wrapped_line));
    }

    // Can't actually change scroll until frame, because might not have updated line wrapping yet
    pub fn setCenterAtPos(self: *Editor, pos: usize) void {
        self.wanted_center_pos = pos;
    }

    fn scrollWrappedLineToCenter(self: *Editor, text_rect: u.Rect, wrapped_line: usize) void {
        const center_pixel = @intCast(isize, wrapped_line) * @intCast(isize, self.app.atlas.char_height);
        self.top_pixel = u.max(0, center_pixel - @divTrunc(text_rect.h, 2));
    }

    fn scrollPosToCenter(self: *Editor, text_rect: u.Rect, pos: usize) void {
        const wrapped_line = self.line_wrapped_buffer.getLineColForPos(u.min(pos, self.buffer.getBufferEnd()))[0];
        self.scrollWrappedLineToCenter(text_rect, wrapped_line);
    }

    fn scrollPosIntoView(self: *Editor, text_rect: u.Rect, pos: usize) void {
        const bottom_pixel = self.top_pixel + text_rect.h;
        const cursor_top_pixel = @intCast(isize, self.line_wrapped_buffer.getLineColForPos(pos)[0]) * @intCast(isize, self.app.atlas.char_height);
        const cursor_bottom_pixel = cursor_top_pixel + @intCast(isize, self.app.atlas.char_height);
        if (cursor_top_pixel > bottom_pixel - @intCast(isize, self.app.atlas.char_height))
            self.top_pixel = cursor_top_pixel - @intCast(isize, text_rect.h) + @intCast(isize, self.app.atlas.char_height);
        if (cursor_bottom_pixel <= self.top_pixel + @intCast(isize, self.app.atlas.char_height))
            self.top_pixel = cursor_top_pixel;
    }

    fn goMatchingParen(self: *Editor) void {
        for (self.cursors.items) |*cursor| {
            const pos = cursor.head.pos;
            if (self.buffer.language.matchParen(pos)) |matching_pos|
                self.goPos(cursor, matching_pos);
        }
    }

    fn deleteToken(self: *Editor) void {
        for (self.cursors.items) |*cursor| {
            const pos = cursor.head.pos;
            if (self.buffer.language.getTokenIxBefore(pos)) |token_ix| {
                const range = self.buffer.language.getTokenRanges()[token_ix];
                self.buffer.delete(range[0], pos);
            }
        }
    }
};
