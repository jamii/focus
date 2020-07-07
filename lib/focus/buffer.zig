const focus = @import("../focus.zig");
usingnamespace focus.common;
const App = focus.App;
const Id = focus.Id;

pub const BufferSource = union(enum) {
    None,
    Filename: []const u8,
};

pub const Buffer = struct {
    app: *App,
    source: BufferSource,
    bytes: ArrayList(u8),

    pub fn initEmpty(app: *App) ! Id {
        return app.putThing(Buffer{
            .app = app,
            .source = .None,
            .bytes = ArrayList(u8).init(app.allocator),
        });
    }

    pub fn initFromFilename(app: *App, filename: []const u8) ! Id {
        var bytes = ArrayList(u8).init(app.allocator);
        errdefer bytes.deinit();
        
        const file = try std.fs.cwd().createFile(filename, .{.read=true, .truncate=false});
        defer file.close();
        
        const chunk_size = 1024;
        var buf = try app.allocator.alloc(u8, chunk_size);
        defer app.allocator.free(buf);
        
        while (true) {
            const len = try file.readAll(buf);
            try bytes.appendSlice(buf[0..len]);
            if (len < chunk_size) break;
        }
        
         return app.putThing(Buffer{
             .app = app,
             .source = .{.Filename = filename},
             .bytes = bytes,
        });
    }

    pub fn deinit(self: *Buffer) void {
        self.bytes.deinit();
        switch (self.source) {
            .None => {},
            .Filename => |filename| self.app.allocator.free(filename),
        }
    }

    pub fn save(self: *Buffer) ! void {
        switch (self.source) {
            .None => {},
            .Filename => |filename| {
                const file = try std.fs.cwd().createFile(filename, .{.read=false, .truncate=true});
                defer file.close();
                try file.writeAll(self.bytes.items);
            }
        }
    }

    pub fn getBufferEnd(self: *Buffer) usize {
        return self.bytes.items.len;
    }

    pub fn getPosForLine(self: *Buffer, line: usize) usize {
        var pos: usize = 0;
        var lines_remaining = line;
        while (lines_remaining > 0) : (lines_remaining -= 1) {
            pos = if (self.searchForwards(pos, "\n")) |next_pos| next_pos + 1 else self.bytes.items.len;
        }
        return pos;
    }

    pub fn getPosForLineCol(self: *Buffer, line: usize, col: usize) usize {
        var pos = self.getPosForLine(line);
        const end = if (self.searchForwards(pos, "\n")) |line_end| line_end else self.bytes.items.len;
        pos += min(col, end - pos);
        return pos;
    }

    pub fn getLineColForPos(self: *Buffer, pos: usize) [2]usize {
        var line: usize = 0;
        const col = pos - (self.searchBackwards(pos, "\n") orelse 0);
        var pos_remaining = pos;
        while (self.searchBackwards(pos_remaining, "\n")) |line_start| {
            pos_remaining = line_start - 1;
            line += 1;
        }
        return .{line, col};
    }

    pub fn searchBackwards(self: *Buffer, pos: usize, needle: []const u8) ?usize {
        const bytes = self.bytes.items[0..pos];
        return if (std.mem.lastIndexOf(u8, bytes, needle)) |result_pos| result_pos + needle.len else null;
    }

    pub fn searchForwards(self: *Buffer, pos: usize, needle: []const u8) ?usize {
        const bytes = self.bytes.items[pos..];
        return if (std.mem.indexOf(u8, bytes, needle)) |result_pos| result_pos + pos else null;
    }

    pub fn getLineStart(self: *Buffer, pos: usize) usize {
        return self.searchBackwards(pos, "\n") orelse 0;
    }

    pub fn getLineEnd(self: *Buffer, pos: usize) usize {
        return self.searchForwards(pos, "\n") orelse self.getBufferEnd();
    }

    pub fn dupe(self: *Buffer, allocator: *Allocator, start: usize, end: usize) ! []const u8 {
        assert(start <= end);
        assert(end <= self.bytes.items.len);
        return std.mem.dupe(allocator, u8, self.bytes.items[start..end]);
    }

    pub fn insert(self: *Buffer, pos: usize, bytes: []const u8) ! void {
        try self.bytes.resize(self.bytes.items.len + bytes.len);
        std.mem.copyBackwards(u8, self.bytes.items[pos+bytes.len..], self.bytes.items[pos..self.bytes.items.len - bytes.len]);
        std.mem.copy(u8, self.bytes.items[pos..], bytes);
    }

    pub fn delete(self: *Buffer, start: usize, end: usize) void {
        assert(start <= end);
        assert(end <= self.bytes.items.len);
        std.mem.copy(u8, self.bytes.items[start..], self.bytes.items[end..]);
        self.bytes.shrink(self.bytes.items.len - (end - start));
    }

    pub fn countLines(self: *Buffer) usize {
        var lines: usize = 0;
        var iter = std.mem.split(self.bytes.items, "\n");
        while (iter.next()) |_| lines += 1;
        return lines;
    }
};
