const std = @import("std");
const focus = @import("../focus.zig");
const u = focus.util;
const c = focus.util.c;
const App = focus.App;
const Editor = focus.Editor;
//const ImpRepl = focus.ImpRepl;
const LineWrappedBuffer = focus.LineWrappedBuffer;
const Language = focus.Language;

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

pub const Options = struct {
    limit_load_bytes: bool = false,
    enable_completions: bool = true,
    enable_undo: bool = true,
};

// rare to have enough space to put more chars than this above the fold
const limited_load_bytes = 200 * 500;

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

    fn deinit(self: Edit, allocator: u.Allocator) void {
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
    language: Language,
    bytes: u.ArrayList(u8),
    undos: u.ArrayList([]Edit),
    doing: u.ArrayList(Edit),
    redos: u.ArrayList([]Edit),
    modified_since_last_save: bool,
    line_ranges: u.ArrayList([2]usize),
    completions: u.ArrayList([]const u8),
    // editors must unregister before buffer deinits
    editors: u.ArrayList(*Editor),
    options: Options,
    last_lost_focus_ms: i64,

    // store cursor head and center pos of last open editor, so if we open a new editor it can start in the same place
    last_cursor_head: usize,
    last_center_pos: usize,

    pub fn initEmpty(app: *App, options: Options) *Buffer {
        const self = app.allocator.create(Buffer) catch u.oom();
        self.* = Buffer{
            .app = app,
            .source = .None,
            .language = .Unknown,
            .bytes = u.ArrayList(u8).init(app.allocator),
            .undos = u.ArrayList([]Edit).init(app.allocator),
            .doing = u.ArrayList(Edit).init(app.allocator),
            .redos = u.ArrayList([]Edit).init(app.allocator),
            .modified_since_last_save = false,
            .line_ranges = u.ArrayList([2]usize).init(app.allocator),
            .completions = u.ArrayList([]const u8).init(app.allocator),
            .editors = u.ArrayList(*Editor).init(app.allocator),
            .options = options,
            .last_lost_focus_ms = 0,
            .last_cursor_head = 0,
            .last_center_pos = 0,
        };
        self.updateLineRanges();
        return self;
    }

    pub fn initFromAbsoluteFilename(app: *App, options: Options, absolute_filename: []const u8) *Buffer {
        u.assert(std.fs.path.isAbsolute(absolute_filename));
        const self = Buffer.initEmpty(app, options);
        self.source = .{
            .File = .{
                .absolute_filename = self.app.allocator.dupe(u8, absolute_filename) catch u.oom(),
                .mtime = 0,
            },
        };
        self.language = Language.fromFilename(absolute_filename);
        self.load(.Init);
        self.undos.resize(0) catch u.oom();
        self.modified_since_last_save = false;
        return self;
    }

    pub fn deinit(self: *Buffer) void {
        // all editors should have unregistered already
        u.assert(self.editors.items.len == 0);
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
        if (self.options.limit_load_bytes) num_bytes = u.min(num_bytes, limited_load_bytes);

        var bytes = self.app.frame_allocator.alloc(u8, num_bytes) catch u.oom();
        const len = try file.readAll(bytes);
        // TODO can this fail if the file was truncated between stat and read?
        u.assert(len == bytes.len);

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
            const message = u.format(self.app.frame_allocator, "{} while loading {s}", .{ err, self.getFilename() });
            std.debug.print("{s}\n", .{message});
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
                        else => u.panic("{} while refreshing {s}", .{ err, file_source.absolute_filename }),
                    }
                };
                defer file.close();
                const stat = file.stat() catch |err| u.panic("{} while refreshing {s}", .{ err, file_source.absolute_filename });
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
                        u.panic("{} while saving {s}", .{ err, file_source.absolute_filename });
                    },
                    .Auto => file: {
                        if (std.fs.cwd().openFile(file_source.absolute_filename, .{ .mode = .write_only })) |file| {
                            file.setEndPos(0) catch |err| u.panic("{} while truncating {s}", .{ err, file_source.absolute_filename });
                            file.seekTo(0) catch |err| u.panic("{} while truncating {s}", .{ err, file_source.absolute_filename });
                            break :file file;
                        } else |err| {
                            switch (err) {
                                // if file has been deleted, only save in response to C-s
                                error.FileNotFound => {
                                    self.modified_since_last_save = true;
                                    return;
                                },
                                else => u.panic("{} while saving {s}", .{ err, file_source.absolute_filename }),
                            }
                        }
                    },
                };
                defer file.close();

                file.writeAll(self.bytes.items) catch |err| u.panic("{} while saving {s}", .{ err, file_source.absolute_filename });
                const stat = file.stat() catch |err| u.panic("{} while saving {s}", .{ err, file_source.absolute_filename });
                file_source.mtime = stat.mtime;
                self.modified_since_last_save = false;

                self.app.handleAfterSave();
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
        return line_range[0] + u.min(col, line_range[1] - line_range[0]);
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

    pub fn searchBackwards(self: *Buffer, pos: usize, needle: []const u8) ?usize {
        const bytes = self.bytes.items[0..pos];
        return if (std.mem.lastIndexOf(u8, bytes, needle)) |result_pos| result_pos + needle.len else null;
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
        return self.searchBackwards(pos, "\n") orelse 0;
    }

    pub fn getLineEnd(self: *Buffer, pos: usize) usize {
        return self.searchForwards(pos, "\n") orelse self.getBufferEnd();
    }

    // TODO pass Writer instead of Allocator for easy concat/sentinel? but costs more allocations?
    pub fn dupe(self: *Buffer, allocator: u.Allocator, start: usize, end: usize) []const u8 {
        u.assert(start <= end);
        u.assert(end <= self.bytes.items.len);
        return allocator.dupe(u8, self.bytes.items[start..end]) catch u.oom();
    }

    fn rawInsert(self: *Buffer, pos: usize, bytes: []const u8) void {
        const line_start = self.getLineStart(pos);
        const line_end = self.getLineEnd(pos);
        self.removeRangeFromCompletions(line_start, line_end);

        self.bytes.resize(self.bytes.items.len + bytes.len) catch u.oom();
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
        u.assert(start <= end);
        u.assert(end <= self.bytes.items.len);

        const line_start = self.getLineStart(start);
        const line_end = self.getLineEnd(end);
        self.removeRangeFromCompletions(line_start, line_end);

        std.mem.copy(u8, self.bytes.items[start..], self.bytes.items[end..]);
        self.bytes.shrinkAndFree(self.bytes.items.len - (end - start));

        self.addRangeToCompletions(line_start, line_end - (end - start));

        self.updateLineRanges();
        self.modified_since_last_save = true;
        for (self.editors.items) |editor| {
            editor.updateAfterDelete(start, end);
        }
    }

    fn rawReplace(self: *Buffer, new_bytes: []const u8) void {
        var line_colss = u.ArrayList([][2][2]usize).init(self.app.frame_allocator);
        for (self.editors.items) |editor| {
            line_colss.append(editor.updateBeforeReplace()) catch u.oom();
        }
        std.mem.reverse([][2][2]usize, line_colss.items);

        self.bytes.resize(0) catch u.oom();
        self.bytes.appendSlice(new_bytes) catch u.oom();

        self.updateCompletions();

        self.updateLineRanges();
        self.modified_since_last_save = true;
        for (self.editors.items) |editor| {
            editor.updateAfterReplace(line_colss.pop());
        }
    }

    pub fn insert(self: *Buffer, pos: usize, bytes: []const u8) void {
        if (self.options.enable_undo) {
            self.doing.append(.{
                .Insert = .{
                    .start = pos,
                    .end = pos + bytes.len,
                    .new_bytes = self.app.allocator.dupe(u8, bytes) catch u.oom(),
                },
            }) catch u.oom();
            for (self.redos.items) |edits| {
                for (edits) |edit| edit.deinit(self.app.allocator);
                self.app.allocator.free(edits);
            }
            self.redos.shrinkAndFree(0);
        }
        self.rawInsert(pos, bytes);
    }

    pub fn delete(self: *Buffer, start: usize, end: usize) void {
        if (self.options.enable_undo) {
            self.doing.append(.{
                .Delete = .{
                    .start = start,
                    .end = end,
                    .old_bytes = self.app.allocator.dupe(u8, self.bytes.items[start..end]) catch u.oom(),
                },
            }) catch u.oom();
            for (self.redos.items) |edits| {
                for (edits) |edit| edit.deinit(self.app.allocator);
                self.app.allocator.free(edits);
            }
            self.redos.shrinkAndFree(0);
        }
        self.rawDelete(start, end);
    }

    pub fn replace(self: *Buffer, new_bytes: []const u8) void {
        if (!std.mem.eql(u8, self.bytes.items, new_bytes)) {
            self.newUndoGroup();
            if (self.options.enable_undo) {
                self.doing.append(.{
                    .Replace = .{
                        .old_bytes = self.app.allocator.dupe(u8, self.bytes.items) catch u.oom(),
                        .new_bytes = self.app.allocator.dupe(u8, new_bytes) catch u.oom(),
                    },
                }) catch u.oom();
                for (self.redos.items) |edits| {
                    for (edits) |edit| edit.deinit(self.app.allocator);
                    self.app.allocator.free(edits);
                }
                self.redos.shrinkAndFree(0);
            }
            self.rawReplace(new_bytes);
            self.newUndoGroup();
        }
    }

    pub fn newUndoGroup(self: *Buffer) void {
        if (self.doing.items.len > 0) {
            const edits = self.doing.toOwnedSlice();
            std.mem.reverse(Edit, edits);
            self.undos.append(edits) catch u.oom();
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
            self.redos.append(edits) catch u.oom();
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
            self.undos.append(edits) catch u.oom();
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

    fn updateLineRanges(self: *Buffer) void {
        var line_ranges = &self.line_ranges;
        const bytes = self.bytes.items;
        const len = bytes.len;

        self.line_ranges.resize(0) catch u.oom();

        var start: usize = 0;
        while (start <= len) {
            var end = start;
            while (end < len and bytes[end] != '\n') : (end += 1) {}
            line_ranges.append(.{ start, end }) catch u.oom();
            start = end + 1;
        }
    }

    fn updateCompletions(self: *Buffer) void {
        if (!self.options.enable_completions) return;

        for (self.completions.items) |completion| self.app.allocator.free(completion);
        self.completions.resize(0) catch u.oom();
        self.completions.appendSlice(self.language.getTokens(self.app.allocator, self.bytes.items, .{ 0, self.bytes.items.len })) catch u.oom();
        std.sort.sort([]const u8, self.completions.items, {}, struct {
            fn lessThan(_: void, a: []const u8, b: []const u8) bool {
                return std.mem.lessThan(u8, a, b);
            }
        }.lessThan);
    }

    // always called with entire lines
    fn removeRangeFromCompletions(self: *Buffer, range_start: usize, range_end: usize) void {
        if (!self.options.enable_completions) return;

        const completion_ranges = self.language.getTokenRanges(self.app.frame_allocator, self.bytes.items, .{ range_start, range_end });
        for (completion_ranges) |completion_range| {
            const completion = self.bytes.items[completion_range[0]..completion_range[1]];
            const pos = u.binarySearch([]const u8, completion, self.completions.items, {}, (struct {
                fn compare(_: void, a: []const u8, b: []const u8) std.math.Order {
                    return std.mem.order(u8, a, b);
                }
            }).compare).Found;
            const removed = self.completions.orderedRemove(pos);
            self.app.allocator.free(removed);
        }
    }

    // always called with entire lines
    fn addRangeToCompletions(self: *Buffer, range_start: usize, range_end: usize) void {
        if (!self.options.enable_completions) return;

        const new_completions = self.language.getTokens(self.app.allocator, self.bytes.items, .{ range_start, range_end });
        for (new_completions) |new_completion| {
            const pos = u.binarySearch([]const u8, new_completion, self.completions.items, {}, (struct {
                fn compare(_: void, a: []const u8, b: []const u8) std.math.Order {
                    return std.mem.order(u8, a, b);
                }
            }).compare).position();
            self.completions.insert(pos, self.app.dupe(new_completion)) catch u.oom();
        }
    }

    pub fn getCompletionsInto(self: *Buffer, prefix: []const u8, results: *u.ArrayList([]const u8)) void {
        // find first completion that might match prefix
        const start = u.binarySearch([]const u8, prefix, self.completions.items, {}, (struct {
            fn compare(_: void, a: []const u8, b: []const u8) std.math.Order {
                return std.mem.order(u8, a, b);
            }
        }).compare).position();

        // search for last completion that matches prefix
        var end = start;
        const completions_items = self.completions.items;
        const len = completions_items.len;
        while (end < len) : (end += 1) {
            const completion = completions_items[end];
            if (!std.mem.startsWith(u8, completion, prefix)) break;
            if (completion.len > prefix.len)
                results.append(completions_items[end]) catch u.oom();
        }
    }

    pub fn getCompletionRange(self: *Buffer, pos: usize) [2]usize {
        return self.language.getTokenRangeAround(self.app.frame_allocator, self.bytes.items, pos) orelse .{ pos, pos };
    }

    pub fn getCompletionPrefix(self: *Buffer, pos: usize) []const u8 {
        const range = self.getCompletionRange(pos);
        return self.bytes.items[range[0]..pos];
    }

    pub fn getCompletionToken(self: *Buffer, pos: usize) []const u8 {
        const range = self.getCompletionRange(pos);
        return self.bytes.items[range[0]..range[1]];
    }

    pub fn insertCompletion(self: *Buffer, pos: usize, completion: []const u8) void {
        const token_range = self.getCompletionRange(pos);
        // (insert before delete so completion gets duped before self.completions updates)
        self.insert(token_range[0], completion);
        self.delete(token_range[0] + completion.len, token_range[1] + completion.len);
    }

    pub fn registerEditor(self: *Buffer, editor: *Editor) void {
        self.editors.append(editor) catch u.oom();
    }

    pub fn deregisterEditor(self: *Buffer, editor: *Editor) void {
        const i = std.mem.indexOfScalar(*Editor, self.editors.items, editor).?;
        _ = self.editors.swapRemove(i);
    }
};
