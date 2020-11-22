const focus = @import("../focus.zig");
usingnamespace focus.common;
const App = focus.App;
const Buffer = focus.Buffer;
const meta = focus.meta;

pub const LineWrappedBuffer = struct {
    app: *App,
    buffer: *Buffer,
    max_chars_per_line: usize,
    wrapped_line_ranges: ArrayList([2]usize),

    pub fn init(app: *App, buffer: *Buffer, max_chars_per_line: usize) LineWrappedBuffer {
        var self = LineWrappedBuffer{
            .app = app,
            .buffer = buffer,
            .max_chars_per_line = max_chars_per_line,
            .wrapped_line_ranges = ArrayList([2]usize).init(app.allocator),
        };
        self.update();
        return self;
    }

    pub fn deinit(self: *LineWrappedBuffer) void {
        self.wrapped_line_ranges.deinit();
    }

    // TODO use buffer.line_ranges to skip work
    pub fn update(self: *LineWrappedBuffer) void {
        self.wrapped_line_ranges.resize(0) catch oom();
        const buffer_end = self.buffer.getBufferEnd();
        var line_start: usize = 0;
        while (true) {
            var line_end = line_start;
            var maybe_line_end: usize = line_end;
            {
                while (true) {
                    if (maybe_line_end >= buffer_end) {
                        line_end = maybe_line_end;
                        break;
                    }
                    const char = self.buffer.getChar(maybe_line_end);
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
                        // commit to including this char
                        line_end = maybe_line_end;
                    }
                    // otherwise keep looking ahead
                }
            }
            self.wrapped_line_ranges.append(.{ line_start, line_end }) catch oom();
            if (line_end >= buffer_end) {
                break;
            }
            line_start = line_end;
            if (line_start < self.buffer.getBufferEnd() and self.buffer.getChar(line_start) == '\n') {
                line_start += 1;
            }
        }
    }

    // TODO binary search and check next line
    pub fn getLineColForPos(self: *LineWrappedBuffer, pos: usize) [2]usize {
        // iterate backwards to resolve ambiguity around putting the cursor before/after line wraps
        var line: usize = self.wrapped_line_ranges.items.len - 1;
        while (line >= 0) : (line -= 1) {
            const line_range = self.wrapped_line_ranges.items[line];
            if (pos >= line_range[0] and pos <= line_range[1]) {
                return .{ line, pos - line_range[0] };
            }
        }
        panic("pos {} outside of buffer", .{pos});
    }

    pub fn getRangeForLine(self: *LineWrappedBuffer, line: usize) [2]usize {
        return self.wrapped_line_ranges.items[line];
    }

    pub fn getPosForLine(self: *LineWrappedBuffer, line: usize) usize {
        return self.getRangeForLine(line)[0];
    }

    pub fn getPosForLineCol(self: *LineWrappedBuffer, line: usize, col: usize) usize {
        const range = self.wrapped_line_ranges.items[line];
        return range[0] + min(col, range[1] - range[0]);
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
