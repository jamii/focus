const std = @import("std");
const focus = @import("../focus.zig");
const u = focus.util;
const c = focus.util.c;
const App = focus.App;
const Buffer = focus.Buffer;

pub const LineWrappedBuffer = struct {
    app: *App,
    buffer: *Buffer,
    max_chars_per_line: usize,
    wrapped_line_ranges: u.ArrayList([2]usize),

    pub fn init(app: *App, buffer: *Buffer, max_chars_per_line: usize) LineWrappedBuffer {
        var self = LineWrappedBuffer{
            .app = app,
            .buffer = buffer,
            .max_chars_per_line = max_chars_per_line,
            .wrapped_line_ranges = u.ArrayList([2]usize).init(app.allocator),
        };
        self.update();
        return self;
    }

    pub fn deinit(self: *LineWrappedBuffer) void {
        self.wrapped_line_ranges.deinit();
    }

    pub fn update(self: *LineWrappedBuffer) void {
        self.wrapped_line_ranges.shrinkRetainingCapacity(0);
        self.updateFromLineRanges(self.buffer.line_ranges.items);
    }

    pub fn updateAfterAppend(self: *LineWrappedBuffer, append_pos: usize) void {
        const line_ranges = self.buffer.line_ranges.items;
        const wrapped_line_ranges = &self.wrapped_line_ranges;

        var line_start_ix = line_ranges.len -| 1;
        while (line_start_ix > 0 and line_ranges[line_start_ix][1] > append_pos) : (line_start_ix -= 1) {}

        const start_pos = line_ranges[line_start_ix][0];
        while (wrapped_line_ranges.popOrNull()) |wrapped_line_range| {
            if (wrapped_line_range[0] < start_pos) {
                wrapped_line_ranges.append(wrapped_line_range) catch u.oom();
                break;
            }
        }

        self.updateFromLineRanges(line_ranges[line_start_ix..]);
    }

    fn updateFromLineRanges(self: *LineWrappedBuffer, line_ranges: []const [2]usize) void {
        const bytes = self.buffer.bytes.items;
        const wrapped_line_ranges = &self.wrapped_line_ranges;
        for (line_ranges) |real_line_range| {
            const real_line_end = real_line_range[1];
            var line_start: usize = real_line_range[0];
            if (real_line_end - line_start <= self.max_chars_per_line) {
                wrapped_line_ranges.append(real_line_range) catch u.oom();
                continue;
            }
            while (true) {
                var line_end = line_start;
                var maybe_line_end = line_end;
                var seen_non_whitespace = false;
                {
                    while (true) {
                        if (maybe_line_end >= real_line_end) {
                            line_end = maybe_line_end;
                            break;
                        }
                        const char = bytes[maybe_line_end];
                        if (maybe_line_end - line_start > self.max_chars_per_line) {
                            // if we haven't soft wrapped yet, hard wrap before this char, otherwise use soft wrap
                            if (line_end == line_start) {
                                line_end = maybe_line_end;
                            }
                            break;
                        }
                        if (char == '\n') {
                            // wrap here
                            line_end = maybe_line_end;
                            break;
                        }
                        maybe_line_end += 1;
                        if (char == ' ') {
                            if (seen_non_whitespace)
                                // commit to including this char
                                line_end = maybe_line_end;
                        } else {
                            seen_non_whitespace = true;
                        }
                        // otherwise keep looking ahead
                    }
                }
                self.wrapped_line_ranges.append(.{ line_start, line_end }) catch u.oom();
                if (line_end >= real_line_end) break;
                line_start = line_end;
            }
        }
    }

    pub fn getLineColForPos(self: *LineWrappedBuffer, pos: usize) [2]usize {
        // TODO avoid hacky fake key
        var line = std.sort.binarySearch([2]usize, [2]usize{ pos, pos }, self.wrapped_line_ranges.items, {}, struct {
            fn compare(_: void, key: [2]usize, item: [2]usize) std.math.Order {
                if (key[0] < item[0]) return .lt;
                if (key[0] > item[1]) return .gt;
                return .eq;
            }
        }.compare).?;
        // check next line to resolve ambiguity around putting the cursor before/after line wraps
        if (line + 1 < self.wrapped_line_ranges.items.len and pos == self.wrapped_line_ranges.items[line + 1][0]) line = line + 1;
        const line_range = self.wrapped_line_ranges.items[line];
        return .{ line, pos - line_range[0] };
    }

    pub fn getRangeForLine(self: *LineWrappedBuffer, line: usize) [2]usize {
        return self.wrapped_line_ranges.items[line];
    }

    pub fn getPosForLine(self: *LineWrappedBuffer, line: usize) usize {
        return self.getRangeForLine(line)[0];
    }

    pub fn getPosForLineCol(self: *LineWrappedBuffer, line: usize, col: usize) usize {
        const range = self.wrapped_line_ranges.items[line];
        return range[0] + @min(col, range[1] - range[0]);
    }

    pub fn getLineStart(self: *LineWrappedBuffer, pos: usize) usize {
        const line_col = self.getLineColForPos(pos);
        return self.getRangeForLine(line_col[0])[0];
    }

    pub fn getLineEnd(self: *LineWrappedBuffer, pos: usize) usize {
        const line_col = self.getLineColForPos(pos);
        return self.getRangeForLine(line_col[0])[1];
    }

    pub fn countLines(self: *LineWrappedBuffer) usize {
        return self.wrapped_line_ranges.items.len;
    }
};
