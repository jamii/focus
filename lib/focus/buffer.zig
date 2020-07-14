const focus = @import("../focus.zig");
usingnamespace focus.common;
const App = focus.App;
const Id = focus.Id;

pub const BufferSource = union(enum) {
    None,
    AbsoluteFilename: []const u8,
};

pub const Buffer = struct {
    app: *App,
    source: BufferSource,
    bytes: ArrayList(u8),

    pub fn initEmpty(app: *App) Id {
        return app.putThing(Buffer{
            .app = app,
            .source = .None,
            .bytes = ArrayList(u8).init(app.allocator),
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
    }

    pub fn load(self: *Buffer, filename: []const u8) void {
        assert(std.fs.path.isAbsolute(filename));

        self.bytes.shrink(0);

        const file = std.fs.cwd().createFile(filename, .{ .read = true, .truncate = false })
            catch |err| panic("{} while loading {s}", .{err, filename});
        defer file.close();

        const chunk_size = 1024;
        var buf = self.app.frame_allocator.alloc(u8, chunk_size) catch oom();

        while (true) {
            const len = file.readAll(buf)
                catch |err| panic("{} while loading {s}", .{err, filename});
            // worth handling oom here for big files
            self.bytes.appendSlice(buf[0..len])
                catch |err| panic("{} while loading {s}", .{err, filename});
            if (len < chunk_size) break;
        }

        self.source = .{ .AbsoluteFilename = std.mem.dupe(self.app.allocator, u8, filename) catch oom() };
    }

    pub fn save(self: *Buffer) void {
        switch (self.source) {
            .None => {},
            .AbsoluteFilename => |filename| {
                const file = std.fs.cwd().createFile(filename, .{ .read = false, .truncate = true })
                    catch |err| panic("{} while saving {s}", .{err, filename});
                defer file.close();
                file.writeAll(self.bytes.items)
                    catch |err| panic("{} while saving {s}", .{err, filename});
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

    pub fn dupe(self: *Buffer, allocator: *Allocator, start: usize, end: usize) []const u8 {
        assert(start <= end);
        assert(end <= self.bytes.items.len);
        return std.mem.dupe(allocator, u8, self.bytes.items[start..end]) catch oom();
    }

    pub fn insert(self: *Buffer, pos: usize, bytes: []const u8) void {
        self.bytes.resize(self.bytes.items.len + bytes.len) catch oom();
        std.mem.copyBackwards(u8, self.bytes.items[pos + bytes.len ..], self.bytes.items[pos .. self.bytes.items.len - bytes.len]);
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
