const focus = @import("../focus.zig");
usingnamespace focus.common;
const App = focus.App;
const Id = focus.Id;
const meta = focus.meta;

pub const BufferSource = union(enum) {
    None,
    AbsoluteFilename: []const u8,
};

pub const Edit = struct {
    tag: enum {
        Insert, Delete
    },
    data: struct {
        start: usize,
        end: usize,
        bytes: []const u8,
    },
};

pub const Buffer = struct {
    app: *App,
    source: BufferSource,
    bytes: ArrayList(u8),
    undos: ArrayList([]Edit),
    doing: ArrayList(Edit),
    redos: ArrayList([]Edit),
    modified_since_last_save: bool,

    pub fn initEmpty(app: *App) Id {
        return app.putThing(Buffer{
            .app = app,
            .source = .None,
            .bytes = ArrayList(u8).init(app.allocator),
            .undos = ArrayList([]Edit).init(app.allocator),
            .doing = ArrayList(Edit).init(app.allocator),
            .redos = ArrayList([]Edit).init(app.allocator),
            .modified_since_last_save = false,
        });
    }

    pub fn initFromAbsoluteFilename(app: *App, filename: []const u8) Id {
        const self_id = Buffer.initEmpty(app);
        var self = app.getThing(self_id).Buffer;
        self.load(filename);
        return self_id;
    }

    pub fn deinit(self: *Buffer) void {
        self.bytes.deinit();
        switch (self.source) {
            .None => {},
            .AbsoluteFilename => |filename| self.app.allocator.free(filename),
        }
        for (self.undos) |edits| for (edits) |edit| self.app.allocator.free(edit.bytes);
        for (self.doing) |edit| self.app.allocator.free(edit.bytes);
        for (self.redos) |edits| for (edits) |edit| self.app.allocator.free(edit.bytes);
    }

    fn rawLoad(self: *Buffer, filename: []const u8) !void {
        assert(std.fs.path.isAbsolute(filename));

        self.bytes.shrink(0);

        const file = try std.fs.cwd().createFile(filename, .{ .read = true, .truncate = false });
        defer file.close();

        const chunk_size = 1024;
        var buf = self.app.frame_allocator.alloc(u8, chunk_size) catch oom();

        while (true) {
            const len = try file.readAll(buf);
            // worth handling oom here for big files
            try self.bytes.appendSlice(buf[0..len]);
            if (len < chunk_size) break;
        }

        self.source = .{ .AbsoluteFilename = std.mem.dupe(self.app.allocator, u8, filename) catch oom() };
    }

    pub fn load(self: *Buffer, filename: []const u8) void {
        self.rawLoad(filename) catch |err| {
            self.bytes.shrink(0);
            // TODO differentiate from actual text
            std.fmt.format(self.bytes.outStream(), "{} while loading {s}", .{ err, filename }) catch oom();
            self.source = .None;
        };
    }

    pub fn save(self: *Buffer) void {
        switch (self.source) {
            .None => {},
            .AbsoluteFilename => |filename| {
                const file = std.fs.cwd().createFile(filename, .{ .read = false, .truncate = true }) catch |err| panic("{} while saving {s}", .{ err, filename });
                defer file.close();
                file.writeAll(self.bytes.items) catch |err| panic("{} while saving {s}", .{ err, filename });
                self.modified_since_last_save = false;
            },
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
        return .{ line, col };
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

    // TODO pass outStream instead of Allocator for easy concat/sentinel? but costs more allocations?
    pub fn dupe(self: *Buffer, allocator: *Allocator, start: usize, end: usize) []const u8 {
        assert(start <= end);
        assert(end <= self.bytes.items.len);
        return std.mem.dupe(allocator, u8, self.bytes.items[start..end]) catch oom();
    }

    fn rawInsert(self: *Buffer, pos: usize, bytes: []const u8) void {
        self.bytes.resize(self.bytes.items.len + bytes.len) catch oom();
        std.mem.copyBackwards(u8, self.bytes.items[pos + bytes.len ..], self.bytes.items[pos .. self.bytes.items.len - bytes.len]);
        std.mem.copy(u8, self.bytes.items[pos..], bytes);
        self.modified_since_last_save = true;
    }

    fn rawDelete(self: *Buffer, start: usize, end: usize) void {
        assert(start <= end);
        assert(end <= self.bytes.items.len);
        std.mem.copy(u8, self.bytes.items[start..], self.bytes.items[end..]);
        self.bytes.shrink(self.bytes.items.len - (end - start));
        self.modified_since_last_save = true;
    }

    pub fn insert(self: *Buffer, pos: usize, bytes: []const u8) void {
        self.doing.append(.{
            .tag = .Insert,
            .data = .{
                .start = pos,
                .end = pos + bytes.len,
                .bytes = std.mem.dupe(self.app.allocator, u8, bytes) catch oom(),
            },
        }) catch oom();
        self.redos.shrink(0);
        self.rawInsert(pos, bytes);
    }

    pub fn delete(self: *Buffer, start: usize, end: usize) void {
        self.doing.append(.{
            .tag = .Delete,
            .data = .{
                .start = start,
                .end = end,
                .bytes = std.mem.dupe(self.app.allocator, u8, self.bytes.items[start..end]) catch oom(),
            },
        }) catch oom();
        self.redos.shrink(0);
        self.rawDelete(start, end);
    }

pub fn newUndoGroup(self: *Buffer) void {
    if (self.doing.items.len > 0) {
        const edits = self.doing.toOwnedSlice();
        std.mem.reverse(Edit, edits);
        self.undos.append(edits) catch oom();
    }
}

    pub fn undo(self: *Buffer) ?usize {
        self.newUndoGroup();
        var pos: ?usize = null;
        if (self.undos.popOrNull()) |edits| {
            for (edits) |edit| {
                switch (edit.tag) {
                    .Insert => {
                        self.rawDelete(edit.data.start, edit.data.end);
                        pos = edit.data.start;
                    },
                    .Delete => {
                        self.rawInsert(edit.data.start, edit.data.bytes);
                        pos = edit.data.end;
                    },
                }
            }
            std.mem.reverse(Edit, edits);
            self.redos.append(edits) catch oom();
        }
        return pos;
    }

    pub fn redo(self: *Buffer) ?usize {
        var pos: ?usize = null;
        if (self.redos.popOrNull()) |edits| {
            for (edits) |edit| {
                switch (edit.tag) {
                    .Insert => {
                        self.rawInsert(edit.data.start, edit.data.bytes);
                        pos = edit.data.end;
                    },
                    .Delete => {
                        self.rawDelete(edit.data.start, edit.data.end);
                        pos = edit.data.start;
                    },
                }
            }
            std.mem.reverse(Edit, edits);
            self.undos.append(edits) catch oom();
        }
        return pos;
    }

    pub fn countLines(self: *Buffer) usize {
        var lines: usize = 0;
        var iter = std.mem.split(self.bytes.items, "\n");
        while (iter.next()) |_| lines += 1;
        return lines;
    }

    pub fn replace(self: *Buffer, new_bytes: []const u8) void {
        if (!std.mem.eql(u8, self.bytes.items, new_bytes)) {
            self.modified_since_last_save = true;
        }
        // TODO undo group
        self.delete(0, self.getBufferEnd());
        self.insert(0, new_bytes);
    }
};
