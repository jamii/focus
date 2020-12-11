const focus = @import("../focus.zig");
usingnamespace focus.common;
const App = focus.App;
const Editor = focus.Editor;
const meta = focus.meta;
const LineWrappedBuffer = focus.LineWrappedBuffer;

pub const BufferSource = union(enum) {
    None,
    File: struct {
        absolute_filename: []const u8,
        mtime: i128,
    },

    fn deinit(self: *BufferSource, app: *App) void {
        switch (self.*) {
            .None => {},
            .File => |file_source| app.allocator.free(file_source.absolute_filename),
        }
    }
};

pub const Role = enum {
    Real,
    Preview,
};

// rare to have enough space to put more chars than this above the fold
const max_preview_bytes = 200 * 500;

const Edit = union(enum) {
    Insert: struct {
        start: usize,
        end: usize,
        new_bytes: []const u8,
    },
    Delete: struct {
        start: usize,
        end: usize,
        old_bytes: []const u8,
    },
    Replace: struct {
        old_bytes: []const u8,
        new_bytes: []const u8,
    },

    fn deinit(self: Edit, allocator: *Allocator) void {
        switch (self) {
            .Insert => |data| allocator.free(data.new_bytes),
            .Delete => |data| allocator.free(data.old_bytes),
            .Replace => |data| {
                allocator.free(data.new_bytes);
                allocator.free(data.old_bytes);
            },
        }
    }
};

pub const Buffer = struct {
    app: *App,
    source: BufferSource,
    bytes: ArrayList(u8),
    undos: ArrayList([]Edit),
    doing: ArrayList(Edit),
    redos: ArrayList([]Edit),
    modified_since_last_save: bool,
    line_ranges: ArrayList([2]usize),
    completions: ArrayList([]const u8),
    // editors must unregister before buffer deinits
    editors: ArrayList(*Editor),
    role: Role,

    pub fn initEmpty(app: *App, role: Role) *Buffer {
        const self = app.allocator.create(Buffer) catch oom();
        self.* = Buffer{
            .app = app,
            .source = .None,
            .bytes = ArrayList(u8).init(app.allocator),
            .undos = ArrayList([]Edit).init(app.allocator),
            .doing = ArrayList(Edit).init(app.allocator),
            .redos = ArrayList([]Edit).init(app.allocator),
            .modified_since_last_save = false,
            .line_ranges = ArrayList([2]usize).init(app.allocator),
            .completions = ArrayList([]const u8).init(app.allocator),
            .editors = ArrayList(*Editor).init(app.allocator),
            .role = role,
        };
        self.updateLineRanges();
        return self;
    }

    pub fn initFromAbsoluteFilename(app: *App, role: Role, absolute_filename: []const u8) *Buffer {
        assert(std.fs.path.isAbsolute(absolute_filename));
        const self = Buffer.initEmpty(app, role);
        self.source = .{
            .File = .{
                .absolute_filename = std.mem.dupe(self.app.allocator, u8, absolute_filename) catch oom(),
                .mtime = 0,
            },
        };
        self.load(.Init);
        self.undos.resize(0) catch oom();
        return self;
    }

    // TODO fn initPreviewFromAbsoluteFilename

    pub fn deinit(self: *Buffer) void {
        // all editors should have unregistered already
        assert(self.editors.items.len == 0);
        self.editors.deinit();

        for (self.completions.items) |completion| self.app.allocator.free(completion);
        self.completions.deinit();

        self.line_ranges.deinit();

        for (self.undos.items) |edits| {
            for (edits) |edit| edit.deinit(self.app.allocator);
            self.app.allocator.free(edits);
        }
        self.undos.deinit();

        for (self.doing.items) |edit| edit.deinit(self.app.allocator);
        self.doing.deinit();

        for (self.redos.items) |edits| {
            for (edits) |edit| edit.deinit(self.app.allocator);
            self.app.allocator.free(edits);
        }
        self.redos.deinit();

        self.bytes.deinit();

        self.source.deinit(self.app);

        self.app.allocator.destroy(self);
    }

    const TryLoadResult = struct {
        bytes: []const u8,
        mtime: i128,
    };
    fn tryLoad(self: *Buffer) !TryLoadResult {
        const file = try std.fs.cwd().openFile(self.source.File.absolute_filename, .{});
        defer file.close();

        const stat = try file.stat();
        var num_bytes = stat.size;
        if (self.role == .Preview) num_bytes = min(num_bytes, max_preview_bytes);

        var bytes = self.app.frame_allocator.alloc(u8, num_bytes) catch oom();
        const len = try file.readAll(bytes);
        // TODO can this fail if the file was truncated between stat and read?
        assert(len == bytes.len);

        return TryLoadResult{
            .bytes = bytes,
            .mtime = stat.mtime,
        };
    }

    fn load(self: *Buffer, kind: enum { Init, Refresh }) void {
        if (self.tryLoad()) |result| {
            switch (kind) {
                .Init => self.rawReplace(result.bytes),
                .Refresh => self.replace(result.bytes),
            }
            self.source.File.mtime = result.mtime;
        } else |err| {
            const message = format(self.app.frame_allocator, "{} while loading {s}", .{ err, self.getFilename() });
            dump(message);
            self.replace(message);
        }
    }

    pub fn refresh(self: *Buffer) void {
        switch (self.source) {
            .None => {},
            .File => |file_source| {
                const file = std.fs.cwd().openFile(file_source.absolute_filename, .{}) catch |err| {
                    switch (err) {
                        // if file has been deleted, leave buffer as is
                        error.FileNotFound => {
                            self.modified_since_last_save = true;
                            return;
                        },
                        else => panic("{} while refreshing {s}", .{ err, file_source.absolute_filename }),
                    }
                };
                defer file.close();
                const stat = file.stat() catch |err| panic("{} while refreshing {s}", .{ err, file_source.absolute_filename });
                if (stat.mtime != file_source.mtime) {
                    self.load(.Refresh);
                }
            },
        }
    }

    pub const SaveSource = enum {
        User,
        Auto,
    };
    pub fn save(self: *Buffer, source: SaveSource) void {
        switch (self.source) {
            .None => {},
            .File => |*file_source| {
                const file = switch (source) {
                    .User => std.fs.cwd().createFile(file_source.absolute_filename, .{ .read = false, .truncate = true }) catch |err| {
                        panic("{} while saving {s}", .{ err, file_source.absolute_filename });
                    },
                    .Auto => file: {
                        if (std.fs.cwd().openFile(file_source.absolute_filename, .{ .read = false, .write = true })) |file| {
                            file.setEndPos(0) catch |err| panic("{} while truncating {s}", .{ err, file_source.absolute_filename });
                            file.seekTo(0) catch |err| panic("{} while truncating {s}", .{ err, file_source.absolute_filename });
                            break :file file;
                        } else |err| {
                            switch (err) {
                                // if file has been deleted, only save in response to C-s
                                error.FileNotFound => {
                                    self.modified_since_last_save = true;
                                    return;
                                },
                                else => panic("{} while saving {s}", .{ err, file_source.absolute_filename }),
                            }
                        }
                    },
                };
                defer file.close();

                file.writeAll(self.bytes.items) catch |err| panic("{} while saving {s}", .{ err, file_source.absolute_filename });
                const stat = file.stat() catch |err| panic("{} while saving {s}", .{ err, file_source.absolute_filename });
                file_source.mtime = stat.mtime;
                self.modified_since_last_save = false;
            },
        }
    }

    pub fn getBufferEnd(self: *Buffer) usize {
        return self.bytes.items.len;
    }

    pub fn getPosForLine(self: *Buffer, line: usize) usize {
        return self.line_ranges.items[line][0];
    }

    // TODO should handle line out of range too?
    /// Panics on line out of range. Handles col out of range by truncating to end of line.
    pub fn getPosForLineCol(self: *Buffer, line: usize, col: usize) usize {
        const line_range = self.line_ranges.items[line];
        return line_range[0] + min(col, line_range[1] - line_range[0]);
    }

    pub fn getLineColForPos(self: *Buffer, pos: usize) [2]usize {
        // TODO avoid hacky fake key
        const line = std.sort.binarySearch([2]usize, [2]usize{ pos, pos }, self.line_ranges.items, {}, struct {
            fn compare(_: void, key: [2]usize, item: [2]usize) std.math.Order {
                if (key[0] < item[0]) return .lt;
                if (key[0] > item[1]) return .gt;
                return .eq;
            }
        }.compare).?;
        const line_range = self.line_ranges.items[line];
        return .{ line, pos - line_range[0] };
    }

    pub fn searchForwards(self: *Buffer, pos: usize, needle: []const u8) ?usize {
        const bytes = self.bytes.items[pos..];
        return if (std.mem.indexOf(u8, bytes, needle)) |result_pos| result_pos + pos else null;
    }

    pub fn isCloseParen(char: u8) bool {
        return char == ')' or char == '}' or char == ']';
    }

    pub fn isOpenParen(char: u8) bool {
        return char == '(' or char == '{' or char == '[';
    }

    fn matchParenBackwards(self: *Buffer, pos: usize) ?usize {
        const bytes = self.bytes.items;
        var num_closing: usize = 0;
        var search_pos = pos;
        while (search_pos > 0) : (search_pos -= 1) {
            const char = bytes[search_pos];
            if (isCloseParen(char)) num_closing += 1;
            if (isOpenParen(char)) num_closing -= 1;
            if (num_closing == 0) return search_pos;
        }
        return null;
    }

    fn matchParenForwards(self: *Buffer, pos: usize) ?usize {
        const bytes = self.bytes.items;
        const len = bytes.len;
        var num_opening: usize = 0;
        var search_pos = pos;
        while (search_pos < len) : (search_pos += 1) {
            const char = bytes[search_pos];
            if (isCloseParen(char)) num_opening -= 1;
            if (isOpenParen(char)) num_opening += 1;
            if (num_opening == 0) return search_pos;
        }
        return null;
    }

    pub fn matchParen(self: *Buffer, pos: usize) ?[2]usize {
        if (pos < self.bytes.items.len and isOpenParen(self.bytes.items[pos]))
            if (self.matchParenForwards(pos)) |matching_pos|
                return [2]usize{ pos, matching_pos };
        if (pos > 0 and isCloseParen(self.bytes.items[pos - 1]))
            if (self.matchParenBackwards(pos - 1)) |matching_pos|
                return [2]usize{ pos - 1, matching_pos };
        return null;
    }

    pub fn getLineStart(self: *Buffer, pos: usize) usize {
        const line = self.getLineColForPos(pos)[0];
        return self.line_ranges.items[line][0];
    }

    pub fn getLineEnd(self: *Buffer, pos: usize) usize {
        const line = self.getLineColForPos(pos)[0];
        return self.line_ranges.items[line][1];
    }

    // TODO pass outStream instead of Allocator for easy concat/sentinel? but costs more allocations?
    pub fn dupe(self: *Buffer, allocator: *Allocator, start: usize, end: usize) []const u8 {
        assert(start <= end);
        assert(end <= self.bytes.items.len);
        return std.mem.dupe(allocator, u8, self.bytes.items[start..end]) catch oom();
    }

    fn rawInsert(self: *Buffer, pos: usize, bytes: []const u8) void {
        const line_start = self.getLineStart(pos);
        const line_end = self.getLineEnd(pos);
        self.removeRangeFromCompletions(line_start, line_end);

        self.bytes.resize(self.bytes.items.len + bytes.len) catch oom();
        std.mem.copyBackwards(u8, self.bytes.items[pos + bytes.len ..], self.bytes.items[pos .. self.bytes.items.len - bytes.len]);
        std.mem.copy(u8, self.bytes.items[pos..], bytes);

        self.addRangeToCompletions(line_start, line_end + bytes.len);

        self.updateLineRanges();
        self.modified_since_last_save = true;
        for (self.editors.items) |editor| {
            editor.updateAfterInsert(pos, bytes);
        }
    }

    fn rawDelete(self: *Buffer, start: usize, end: usize) void {
        assert(start <= end);
        assert(end <= self.bytes.items.len);

        const line_start = self.getLineStart(start);
        const line_end = self.getLineEnd(end);
        self.removeRangeFromCompletions(line_start, line_end);

        std.mem.copy(u8, self.bytes.items[start..], self.bytes.items[end..]);
        self.bytes.shrink(self.bytes.items.len - (end - start));

        self.addRangeToCompletions(line_start, line_end - (end - start));

        self.updateLineRanges();
        self.modified_since_last_save = true;
        for (self.editors.items) |editor| {
            editor.updateAfterDelete(start, end);
        }
    }

    fn rawReplace(self: *Buffer, new_bytes: []const u8) void {
        var line_colss = ArrayList([][2]usize).init(self.app.frame_allocator);
        for (self.editors.items) |editor| {
            line_colss.append(editor.updateBeforeReplace()) catch oom();
        }
        std.mem.reverse([][2]usize, line_colss.items);

        self.bytes.resize(0) catch oom();
        self.bytes.appendSlice(new_bytes) catch oom();

        self.updateLineRanges();
        self.modified_since_last_save = true;
        for (self.editors.items) |editor| {
            editor.updateAfterReplace(line_colss.pop());
        }
        self.updateCompletions();
    }

    pub fn insert(self: *Buffer, pos: usize, bytes: []const u8) void {
        self.doing.append(.{
            .Insert = .{
                .start = pos,
                .end = pos + bytes.len,
                .new_bytes = std.mem.dupe(self.app.allocator, u8, bytes) catch oom(),
            },
        }) catch oom();
        for (self.redos.items) |edits| {
            for (edits) |edit| edit.deinit(self.app.allocator);
            self.app.allocator.free(edits);
        }
        self.redos.shrink(0);
        self.rawInsert(pos, bytes);
    }

    pub fn delete(self: *Buffer, start: usize, end: usize) void {
        self.doing.append(.{
            .Delete = .{
                .start = start,
                .end = end,
                .old_bytes = std.mem.dupe(self.app.allocator, u8, self.bytes.items[start..end]) catch oom(),
            },
        }) catch oom();
        for (self.redos.items) |edits| {
            for (edits) |edit| edit.deinit(self.app.allocator);
            self.app.allocator.free(edits);
        }
        self.redos.shrink(0);
        self.rawDelete(start, end);
    }

    pub fn replace(self: *Buffer, new_bytes: []const u8) void {
        if (!std.mem.eql(u8, self.bytes.items, new_bytes)) {
            self.newUndoGroup();
            self.doing.append(.{
                .Replace = .{
                    .old_bytes = std.mem.dupe(self.app.allocator, u8, self.bytes.items) catch oom(),
                    .new_bytes = std.mem.dupe(self.app.allocator, u8, new_bytes) catch oom(),
                },
            }) catch oom();
            for (self.redos.items) |edits| {
                for (edits) |edit| edit.deinit(self.app.allocator);
                self.app.allocator.free(edits);
            }
            self.redos.shrink(0);
            self.rawReplace(new_bytes);
            self.newUndoGroup();
        }
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
                switch (edit) {
                    .Insert => |data| {
                        self.rawDelete(data.start, data.end);
                        pos = data.start;
                    },
                    .Delete => |data| {
                        self.rawInsert(data.start, data.old_bytes);
                        pos = data.end;
                    },
                    .Replace => |data| {
                        self.rawReplace(data.old_bytes);
                        // don't set pos
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
                switch (edit) {
                    .Insert => |data| {
                        self.rawInsert(data.start, data.new_bytes);
                        pos = data.end;
                    },
                    .Delete => |data| {
                        self.rawDelete(data.start, data.end);
                        pos = data.start;
                    },
                    .Replace => |data| {
                        self.rawReplace(data.new_bytes);
                        // don't set pos
                    },
                }
            }
            std.mem.reverse(Edit, edits);
            self.undos.append(edits) catch oom();
        }
        return pos;
    }

    pub fn countLines(self: *Buffer) usize {
        return self.line_ranges.items.len;
    }

    pub fn getFilename(self: *Buffer) ?[]const u8 {
        return switch (self.source) {
            .None => null,
            .File => |file_source| file_source.absolute_filename,
        };
    }

    pub fn getChar(self: *Buffer, pos: usize) u8 {
        return self.bytes.items[pos];
    }

    fn isLikeIdent(byte: u8) bool {
        return (byte >= 'a' and byte <= 'z') or (byte >= 'A' and byte <= 'Z') or (byte >= '0' and byte <= '9') or (byte == '_');
    }

    fn updateLineRanges(self: *Buffer) void {
        var line_ranges = &self.line_ranges;
        const bytes = self.bytes.items;
        const len = bytes.len;

        self.line_ranges.resize(0) catch oom();

        var start: usize = 0;
        while (start <= len) {
            var end = start;
            while (end < len and bytes[end] != '\n') : (end += 1) {}
            line_ranges.append(.{ start, end }) catch oom();
            start = end + 1;
        }
    }

    fn updateCompletions(self: *Buffer) void {
        if (self.role == .Preview) return;

        for (self.completions.items) |completion| self.app.allocator.free(completion);
        self.completions.resize(0) catch oom();

        const bytes = self.bytes.items;
        const len = bytes.len;
        const completions = &self.completions;
        var start: usize = 0;
        while (start < len) {
            var end = start;
            while (end < len and isLikeIdent(bytes[end])) : (end += 1) {}
            if (end > start) completions.append(self.app.dupe(bytes[start..end])) catch oom();
            start = end + 1;
            while (start < len and !isLikeIdent(bytes[start])) : (start += 1) {}
        }

        std.sort.sort([]const u8, completions.items, {}, struct {
            fn lessThan(_: void, a: []const u8, b: []const u8) bool {
                return std.mem.lessThan(u8, a, b);
            }
        }.lessThan);
    }

    fn removeRangeFromCompletions(self: *Buffer, range_start: usize, range_end: usize) void {
        const bytes = self.bytes.items;
        const completions = &self.completions;
        var start = range_start;
        while (start < range_end) {
            var end = start;
            while (end < range_end and isLikeIdent(bytes[end])) : (end += 1) {}
            if (end > start) {
                const completions_items = completions.items;
                const completion = bytes[start..end];
                var left: usize = 0;
                var right: usize = completions_items.len;

                const pos = pos: {
                    while (left < right) {
                        const mid = left + (right - left) / 2;
                        switch (std.mem.order(u8, completion, completions_items[mid])) {
                            .eq => break :pos mid,
                            .gt => left = mid + 1,
                            .lt => right = mid,
                        }
                    }
                    // completion should definitely exist in the list
                    @panic("how");
                };

                const removed = completions.orderedRemove(pos);
                self.app.allocator.free(removed);
            }
            start = end + 1;
            while (start < range_end and !isLikeIdent(bytes[start])) : (start += 1) {}
        }
    }

    fn addRangeToCompletions(self: *Buffer, range_start: usize, range_end: usize) void {
        const bytes = self.bytes.items;
        const completions = &self.completions;
        var start = range_start;
        while (start < range_end) {
            var end = start;
            while (end < range_end and isLikeIdent(bytes[end])) : (end += 1) {}
            if (end > start) {
                const completions_items = completions.items;
                const completion = bytes[start..end];
                var left: usize = 0;
                var right: usize = completions_items.len;

                const pos = pos: {
                    while (left < right) {
                        const mid = left + (right - left) / 2;
                        switch (std.mem.order(u8, completion, completions_items[mid])) {
                            .eq => break :pos mid,
                            .gt => left = mid + 1,
                            .lt => right = mid,
                        }
                    }
                    // completion might not be in the list, but there is where it should be added
                    break :pos left;
                };

                completions.insert(pos, self.app.dupe(completion)) catch oom();
            }
            start = end + 1;
            while (start < range_end and !isLikeIdent(bytes[start])) : (start += 1) {}
        }
    }

    pub fn getCompletionsInto(self: *Buffer, prefix: []const u8, results: *ArrayList([]const u8)) void {
        const bytes = self.bytes.items;
        const completions = &self.completions;
        const completions_items = completions.items;
        var left: usize = 0;
        var right: usize = completions_items.len;

        const start = pos: {
            while (left < right) {
                const mid = left + (right - left) / 2;
                switch (std.mem.order(u8, prefix, completions_items[mid])) {
                    .eq => break :pos mid,
                    .gt => left = mid + 1,
                    .lt => right = mid,
                }
            }
            // prefix might not be in the list, but there is where suffixes of it might start
            break :pos left;
        };

        var end = start;
        const len = completions_items.len;
        while (end < len and std.mem.startsWith(u8, completions_items[end], prefix)) : (end += 1) {
            if (end == 0 or !std.mem.eql(u8, completions_items[end - 1], completions_items[end]))
                if (!std.mem.eql(u8, prefix, completions_items[end]))
                    results.append(completions_items[end]) catch oom();
        }
    }

    pub fn getCompletionsPrefix(self: *Buffer, pos: usize) []const u8 {
        const bytes = self.bytes.items;
        var start = pos;
        while (start > 0 and isLikeIdent(bytes[start - 1])) : (start -= 1) {}
        return bytes[start..pos];
    }

    pub fn getCompletionsToken(self: *Buffer, pos: usize) []const u8 {
        const bytes = self.bytes.items;
        const len = bytes.len;
        var start = pos;
        while (start > 0 and isLikeIdent(bytes[start - 1])) : (start -= 1) {}
        var end = pos;
        while (end < len and isLikeIdent(bytes[end])) : (end += 1) {}
        return bytes[start..end];
    }

    pub fn insertCompletion(self: *Buffer, pos: usize, completion: []const u8) void {
        const bytes = self.bytes.items;
        const len = bytes.len;

        // get range of current token
        var start = pos;
        while (start > 0 and isLikeIdent(bytes[start - 1])) : (start -= 1) {}
        var end = pos;
        while (end < len and isLikeIdent(bytes[end])) : (end += 1) {}

        // replace completion
        // (insert before delete so completion gets duped before self.completions updates)
        self.insert(start, completion);
        self.delete(start + completion.len, end + completion.len);
    }

    pub fn registerEditor(self: *Buffer, editor: *Editor) void {
        self.editors.append(editor) catch oom();
    }

    pub fn deregisterEditor(self: *Buffer, editor: *Editor) void {
        const i = std.mem.indexOfScalar(*Editor, self.editors.items, editor).?;
        _ = self.editors.swapRemove(i);
    }
};
