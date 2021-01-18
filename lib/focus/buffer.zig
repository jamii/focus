const focus = @import("../focus.zig");
usingnamespace focus.common;
const App = focus.App;
const Editor = focus.Editor;
const meta = focus.meta;
const BufferTree = focus.BufferTree;
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
    tree: BufferTree,
    undos: ArrayList([]Edit),
    doing: ArrayList(Edit),
    redos: ArrayList([]Edit),
    modified_since_last_save: bool,
    completions: ArrayList([]const u8),
    // editors must unregister before buffer deinits
    editors: ArrayList(*Editor),
    role: Role,

    pub fn initEmpty(app: *App, role: Role) *Buffer {
        const self = app.allocator.create(Buffer) catch oom();
        self.* = Buffer{
            .app = app,
            .source = .None,
            .tree = BufferTree.init(app.allocator),
            .undos = ArrayList([]Edit).init(app.allocator),
            .doing = ArrayList(Edit).init(app.allocator),
            .redos = ArrayList([]Edit).init(app.allocator),
            .modified_since_last_save = false,
            .completions = ArrayList([]const u8).init(app.allocator),
            .editors = ArrayList(*Editor).init(app.allocator),
            .role = role,
        };
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

        self.tree.deinit();

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
                // TODO load directly into a tree?
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

                self.tree.writeInto(file.writer(), 0, self.tree.getTotalBytes()) catch |err| panic("{} while saving {s}", .{ err, file_source.absolute_filename });
                const stat = file.stat() catch |err| panic("{} while saving {s}", .{ err, file_source.absolute_filename });
                file_source.mtime = stat.mtime;
                self.modified_since_last_save = false;
            },
        }
    }

    pub fn getBufferEnd(self: *Buffer) usize {
        return self.tree.getTotalBytes();
    }

    pub fn getPosForLine(self: *Buffer, line: usize) usize {
        return self.tree.getPointForLineStart(line).?.pos;
    }

    /// Handles line out of range by returning end of file.
    /// Handles col out of range by returning end of line.
    pub fn getPosForLineCol(self: *Buffer, line: usize, col: usize) usize {
        const start_point = self.tree.getPointForLineStart(line) orelse return self.getBufferEnd();
        var end_point = start_point;
        _ = end_point.searchForwards("\n");
        return start_point.pos + min(col, end_point.pos - start_point.pos);
    }

    pub fn getLineColForPos(self: *Buffer, pos: usize) [2]usize {
        var point = self.tree.getPointForPos(pos).?;
        if (point.searchBackwards("\n") == .Found) _ = point.seekNextItem();
        return .{ BufferTree.getLine(point), pos - point.pos };
    }

    pub fn searchForwards(self: *Buffer, pos: usize, needle: []const u8) ?usize {
        return self.tree.searchForwards(pos, needle);
    }

    pub fn isCloseParen(char: u8) bool {
        return char == ')' or char == '}' or char == ']';
    }

    pub fn isOpenParen(char: u8) bool {
        return char == '(' or char == '{' or char == '[';
    }

    fn matchParenBackwards(self: *Buffer, pos: usize) ?usize {
        var point = self.tree.getPointForPos(pos).?;
        var num_closing: usize = 0;
        while (true) {
            if (point.seekPrevItem() == .NotFound) break;
            const char = point.getNextItem();
            if (isCloseParen(char)) num_closing += 1;
            if (isOpenParen(char)) num_closing -= 1;
            if (num_closing == 0) return point.pos;
        }
        return null;
    }

    fn matchParenForwards(self: *Buffer, pos: usize) ?usize {
        var point = self.tree.getPointForPos(pos).?;
        var num_opening: usize = 0;
        while (true) {
            const char = point.getNextItem();
            if (isCloseParen(char)) num_opening -= 1;
            if (isOpenParen(char)) num_opening += 1;
            if (num_opening == 0) return point.pos;
            if (point.seekNextItem() == .NotFound) break;
        }
        return null;
    }

    pub fn matchParen(self: *Buffer, pos: usize) ?[2]usize {
        var point = self.tree.getPointForPos(pos).?;
        if (pos < self.getBufferEnd()) {
            if (isOpenParen(point.getNextItem()))
                if (self.matchParenForwards(pos)) |matching_pos|
                    return [2]usize{ pos, matching_pos };
        }
        if (pos > 0) {
            _ = point.seekPrevItem();
            if (isCloseParen(point.getNextItem()))
                if (self.matchParenBackwards(pos)) |matching_pos|
                    return [2]usize{ pos - 1, matching_pos };
        }
        return null;
    }

    pub fn getLineStart(self: *Buffer, pos: usize) usize {
        var point = self.tree.getPointForPos(pos).?;
        if (point.searchBackwards("\n") == .Found) _ = point.seekNextItem();
        return point.pos;
    }

    pub fn getLineEnd(self: *Buffer, pos: usize) usize {
        var point = self.tree.getPointForPos(pos).?;
        _ = point.searchForwards("\n");
        return point.pos;
    }

    pub fn copy(self: *Buffer, allocator: *Allocator, start: usize, end: usize) []const u8 {
        return self.tree.copy(allocator, start, end);
    }

    fn rawInsert(self: *Buffer, pos: usize, bytes: []const u8) void {
        const line_start = self.getLineStart(pos);
        const line_end = self.getLineEnd(pos);
        self.removeRangeFromCompletions(line_start, line_end);

        self.tree.insert(pos, bytes);

        self.addRangeToCompletions(line_start, line_end + bytes.len);

        self.modified_since_last_save = true;
        for (self.editors.items) |editor| {
            editor.updateAfterInsert(pos, bytes);
        }
    }

    fn rawDelete(self: *Buffer, start: usize, end: usize) void {
        assert(start <= end);
        assert(end <= self.getBufferEnd());

        const line_start = self.getLineStart(start);
        const line_end = self.getLineEnd(end);
        self.removeRangeFromCompletions(line_start, line_end);

        self.tree.delete(start, end);

        self.addRangeToCompletions(line_start, line_end - (end - start));

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

        self.tree.deinit();
        self.tree = BufferTree.init(self.app.allocator);
        self.tree.insert(0, new_bytes);

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
                .old_bytes = self.tree.copy(self.app.allocator, start, end),
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
        self.newUndoGroup();
        self.doing.append(.{
            .Replace = .{
                .old_bytes = self.tree.copy(self.app.allocator, 0, self.tree.getTotalBytes()),
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
        return self.tree.getTotalNewlines() + 1;
    }

    pub fn getFilename(self: *Buffer) ?[]const u8 {
        return switch (self.source) {
            .None => null,
            .File => |file_source| file_source.absolute_filename,
        };
    }

    fn isLikeIdent(byte: u8) bool {
        return (byte >= 'a' and byte <= 'z') or (byte >= 'A' and byte <= 'Z') or (byte >= '0' and byte <= '9') or (byte == '_');
    }

    fn updateCompletions(self: *Buffer) void {
        //if (self.role == .Preview) return;
        //
        //const completions = &self.completions;
        //for (completions.items) |completion| self.app.allocator.free(completion);
        //completions.resize(0) catch oom();
        //
        //var point = self.tree.getPointForPos(0).?;
        //while (!point.isAtEnd()) {
        //const start = point.pos;
        //while (!point.isAtEnd() and isLikeIdent(point.getNextItem())) : (_ = point.seekNextItem()) {}
        //if (point.pos > start)
        //completions.append(self.tree.copy(self.app.allocator, start, point.pos)) catch oom();
        //while (!point.isAtEnd() and !isLikeIdent(point.getNextItem())) : (_ = point.seekNextItem()) {}
        //}
        //
        //std.sort.sort([]const u8, completions.items, {}, struct {
        //fn lessThan(_: void, a: []const u8, b: []const u8) bool {
        //return std.mem.lessThan(u8, a, b);
        //}
        //}.lessThan);
    }

    fn removeRangeFromCompletions(self: *Buffer, range_start: usize, range_end: usize) void {
        //const completions = &self.completions;
        //var point = self.tree.getPointForPos(range_start).?;
        //while (point.pos < range_end) {
        //const start = point.pos;
        //while (point.pos < range_end and isLikeIdent(point.getNextItem())) : (_ = point.seekNextItem()) {}
        //if (point.pos > start) {
        //const completions_items = completions.items;
        //const completion = self.tree.copy(self.app.frame_allocator, start, point.pos);
        //var left: usize = 0;
        //var right: usize = completions_items.len;
        //
        //const pos = pos: {
        //while (left < right) {
        //const mid = left + (right - left) / 2;
        //switch (std.mem.order(u8, completion, completions_items[mid])) {
        //.eq => break :pos mid,
        //.gt => left = mid + 1,
        //.lt => right = mid,
        //}
        //}
        //// completion should definitely exist in the list
        //@panic("Tried to remove non-existent completion");
        //};
        //
        //const removed = completions.orderedRemove(pos);
        //self.app.allocator.free(removed);
        //}
        //while (point.pos < range_end and !isLikeIdent(point.getNextItem())) : (_ = point.seekNextItem()) {}
        //}
    }

    fn addRangeToCompletions(self: *Buffer, range_start: usize, range_end: usize) void {
        //const completions = &self.completions;
        //var point = self.tree.getPointForPos(range_start).?;
        //while (point.pos < range_end) {
        //const start = point.pos;
        //while (point.pos < range_end and isLikeIdent(point.getNextItem())) : (_ = point.seekNextItem()) {}
        //if (point.pos > start) {
        //const completions_items = completions.items;
        //const completion = self.tree.copy(self.app.allocator, start, point.pos);
        //var left: usize = 0;
        //var right: usize = completions_items.len;
        //
        //const pos = pos: {
        //while (left < right) {
        //const mid = left + (right - left) / 2;
        //switch (std.mem.order(u8, completion, completions_items[mid])) {
        //.eq => break :pos mid,
        //.gt => left = mid + 1,
        //.lt => right = mid,
        //}
        //}
        //// completion might not be in the list, but this is where it should be added
        //break :pos left;
        //};
        //
        //completions.insert(pos, completion) catch oom();
        //}
        //while (point.pos < range_end and !isLikeIdent(point.getNextItem())) : (_ = point.seekNextItem()) {}
        //}
    }

    pub fn getCompletionsInto(self: *Buffer, prefix: []const u8, results: *ArrayList([]const u8)) void {
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
        var point = self.tree.getPointForPos(pos).?;
        const start = start: {
            while (true) {
                if (point.seekPrevItem() == .NotFound)
                    break :start point.pos;
                if (!isLikeIdent(point.getNextItem()))
                    break :start point.pos + 1;
            }
        };
        return self.tree.copy(self.app.frame_allocator, start, pos);
    }

    pub fn getCompletionsToken(self: *Buffer, pos: usize) []const u8 {
        var start_point = self.tree.getPointForPos(pos).?;
        var end_point = start_point;

        const start = start: {
            while (true) {
                if (start_point.seekPrevItem() == .NotFound)
                    break :start start_point.pos;
                if (!isLikeIdent(start_point.getNextItem()))
                    break :start start_point.pos + 1;
            }
        };

        while (!end_point.isAtEnd() and isLikeIdent(end_point.getNextItem())) : (_ = end_point.seekNextItem()) {}
        const end = end_point.pos;

        return self.tree.copy(self.app.frame_allocator, start, end);
    }

    pub fn insertCompletion(self: *Buffer, pos: usize, completion: []const u8) void {
        // get range of current token
        var start_point = self.tree.getPointForPos(pos).?;
        var end_point = start_point;
        const start = start: {
            while (true) {
                if (start_point.seekPrevItem() == .NotFound)
                    break :start start_point.pos;
                if (!isLikeIdent(start_point.getNextItem()))
                    break :start start_point.pos + 1;
            }
        };
        while (!end_point.isAtEnd() and isLikeIdent(end_point.getNextItem())) : (_ = end_point.seekNextItem()) {}
        const end = end_point.pos;

        // replace completion
        self.delete(start, end);
        self.insert(start, completion);
    }

    pub fn registerEditor(self: *Buffer, editor: *Editor) void {
        self.editors.append(editor) catch oom();
    }

    pub fn deregisterEditor(self: *Buffer, editor: *Editor) void {
        const i = std.mem.indexOfScalar(*Editor, self.editors.items, editor).?;
        _ = self.editors.swapRemove(i);
    }
};
