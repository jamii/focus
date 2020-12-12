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

    pub fn update(self: *LineWrappedBuffer) void {
        const wrapped_line_ranges = &self.wrapped_line_ranges;
        wrapped_line_ranges.resize(0) catch oom();
        for (self.buffer.line_ranges.items) |real_line_range, real_line| {
            if (real_line_range[1] - real_line_range[0] <= self.max_chars_per_line) {
                wrapped_line_ranges.append(real_line_range) catch oom();
                continue;
            }
            const real_line_end = real_line_range[1];
            var line_start = real_line_range[0];
            while (true) {
                var line_end = line_start;
                var maybe_line_end = self.buffer.tree.getPointForPos(line_end).?;
                {
                    while (true) {
                        if (maybe_line_end.pos >= real_line_end) {
                            line_end = maybe_line_end.pos;
                            break;
                        }
                        const char = maybe_line_end.getNextByte();
                        if (maybe_line_end.pos - line_start > self.max_chars_per_line) {
                            // if we haven't soft wrapped yet, hard wrap before this char, otherwise use soft wrap
                            if (line_end == line_start) {
                                line_end = maybe_line_end.pos;
                            }
                            break;
                        }
                        if (char == '\n') {
                            // wrap here
                            line_end = maybe_line_end.pos;
                            break;
                        }
                        _ = maybe_line_end.seekNextByte();
                        if (char == ' ') {
                            // commit to including this char
                            line_end = maybe_line_end.pos;
                        }
                        // otherwise keep looking ahead
                    }
                }
                self.wrapped_line_ranges.append(.{ line_start, line_end }) catch oom();
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
