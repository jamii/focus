const focus = @import("../focus.zig");
usingnamespace focus.common;
const meta = focus.meta;
const Tree = focus.Tree;
const TreeConfig = focus.TreeConfig;

const BufferTreeInner = buffer_tree_inner: {
    var config = TreeConfig{
        .children_per_branch = 128,
        .BranchState = BranchState,
        .items_per_leaf = 4 * 1024,
        .LeafState = LeafState,
        .Item = u8,
    };
    while (@sizeOf(Tree(config).Branch) > 4 * 1024) {
        config.children_per_branch -= 1;
    }
    while (@sizeOf(Tree(config).Leaf) > 4 * 1024) {
        config.items_per_leaf -= 1;
    }
    break :buffer_tree_inner Tree(config);
};

// manually break this circular dependency
const LeafOffset = u16;
comptime {
    assert(BufferTreeInner.Leaf.Offset == LeafOffset);
}

const BranchState = struct {
    num_bytes: usize,
    num_newlines: usize,

    pub fn init() BranchState {
        return .{
            .num_bytes = 0,
            .num_newlines = 0,
        };
    }

    pub fn update(self: *BranchState, node: *BufferTreeInner.Node) void {
        switch (node.tag) {
            .Leaf => {
                const leaf = node.asLeaf();
                self.num_bytes = leaf.num_items;
                self.num_newlines = leaf.state.newlines.items.len;
            },
            .Branch => {
                const branch = node.asBranch();
                self.num_bytes = 0;
                self.num_newlines = 0;
                for (branch.state[0..branch.num_children]) |state| {
                    self.num_bytes += state.num_bytes;
                    self.num_newlines += state.num_newlines;
                }
            },
        }
    }
};

const LeafState = struct {
    newlines: ArrayList(LeafOffset),

    pub fn init(allocator: *Allocator) LeafState {
        return .{
            .newlines = ArrayList(LeafOffset).init(allocator),
        };
    }

    pub fn deinit(self: *LeafState, allocator: *Allocator) void {
        self.newlines.deinit();
    }

    pub fn update(self: *LeafState, leaf: *const BufferTreeInner.Leaf) void {
        self.newlines.resize(0) catch oom();
        for (leaf.items) |char, i|
            if (char == '\n')
                self.newlines.append(@intCast(LeafOffset, i)) catch oom();
    }
};

pub const BufferTree = struct {
    inner: BufferTreeInner,

    pub fn init(allocator: *Allocator) BufferTree {
        return .{
            .inner = BufferTreeInner.init(allocator),
        };
    }

    pub fn deinit(self: *BufferTree) void {
        self.inner.deinit();
    }

    pub fn insert(self: *BufferTree, pos: usize, bytes: []const u8) void {
        var point = self.getPointForPos(pos).?;
        self.inner.insert(&point, bytes);
    }

    pub fn delete(self: *BufferTree, start: usize, end: usize) void {
        var point = self.getPointForPos(start).?;
        self.inner.delete(&point, end - start);
    }

    pub fn copy(self: *BufferTree, allocator: *Allocator, start: usize, end: usize) []const u8 {
        var point = self.getPointForPos(start).?;
        return self.inner.copy(allocator, &point, end - start);
    }

    pub fn getPointForPos(self: BufferTree, pos: usize) ?BufferTreeInner.Point {
        var node = &self.inner.root.node;
        var pos_remaining = pos;
        node: while (node.tag == .Branch) {
            const branch = node.asBranch();
            var child_ix: usize = 0;
            while (true) {
                const num_child_bytes = branch.state[child_ix].num_bytes;
                if (pos_remaining < num_child_bytes) {
                    node = branch.children[child_ix];
                    continue :node;
                }
                child_ix += 1;
                if (child_ix == branch.num_children) {
                    node = branch.children[child_ix - 1];
                    continue :node;
                }
                pos_remaining -= num_child_bytes;
            }
        }
        const leaf = node.asLeaf();

        return if (pos_remaining > leaf.num_items)
            null
        else
            .{
                .pos = pos,
                .leaf = leaf,
                .offset = @intCast(BufferTreeInner.Leaf.Offset, pos_remaining),
            };
    }

    pub fn getPointForLineStart(self: BufferTree, line: usize) ?BufferTreeInner.Point {
        var node = &self.inner.root.node;
        var pos: usize = 0;
        var lines_remaining = line;
        node: while (node.tag == .Branch) {
            const branch = node.asBranch();
            var child_ix: usize = 0;
            while (true) {
                if (lines_remaining <= branch.state[child_ix].num_newlines) {
                    node = branch.children[child_ix];
                    continue :node;
                }
                child_ix += 1;
                if (child_ix == branch.num_children) {
                    node = branch.children[child_ix - 1];
                    continue :node;
                }
                lines_remaining -= branch.state[child_ix - 1].num_newlines;
                pos += branch.state[child_ix - 1].num_bytes;
            }
        }
        const leaf = node.asLeaf();

        if (lines_remaining > leaf.state.newlines.items.len)
            return null;
        const offset = leaf.state.newlines.items[lines_remaining] + 1;

        var point = BufferTreeInner.Point{
            .pos = pos + offset,
            .leaf = leaf,
            .offset = offset,
        };

        // if we're right at the end of the leaf, try going to the start of the next leaf to maintain Point invariants
        if (offset == leaf.num_items and leaf.num_items > 0) {
            _ = point.seekPrevItem();
            _ = point.seekNextItem();
        }

        return point;
    }

    pub fn searchForwards(self: BufferTree, start: usize, needle: []const u8) ?usize {
        var point = self.getPointForPos(start).?;
        switch (point.searchForwards(needle)) {
            .Found => return point.pos,
            .NotFound => return null,
        }
    }

    pub fn searchBackwards(self: BufferTree, start: usize, needle: []const u8) ?usize {
        var point = self.getPointForPos(start).?;
        switch (point.searchBackwards(needle)) {
            .Found => return point.pos,
            .NotFound => return null,
        }
    }

    pub fn getLine(point: BufferTreeInner.Point) usize {
        var line: usize = 0;
        for (point.leaf.state.newlines.items) |offset| {
            if (offset > point.offset) break;
            line += 1;
        }
        var branch = point.leaf.node.getParent().?;
        var child_ix = branch.findChild(&point.leaf.node);
        while (true) {
            for (branch.state[0..child_ix]) |state|
                line += state.num_newlines;
            if (branch.node.getParent()) |parent| {
                child_ix = parent.findChild(&branch.node);
                branch = parent;
            } else {
                break;
            }
        }
        return line;
    }

    pub fn writeInto(self: BufferTree, writer: anytype, start: usize, end: usize) !void {
        var point = self.getPointForPos(start).?;

        var num_remaining_write_items = end - start;
        while (true) {
            const num_write_items = min(num_remaining_write_items, point.leaf.num_items - point.offset);
            try writer.writeAll(point.leaf.items[point.offset .. point.offset + num_write_items]);
            num_remaining_write_items -= num_write_items;

            if (num_remaining_write_items == 0) break;

            assert(point.seekNextLeaf() != .NotFound);
        }
    }

    fn testEqual(self: BufferTree, input: []const u8) void {
        var output = ArrayList(u8).initCapacity(std.testing.allocator, input.len) catch oom();
        defer output.deinit();
        self.writeInto(output.writer(), 0, self.getTotalBytes()) catch unreachable;
        var i: usize = 0;
        while (i < min(input.len, output.items.len)) : (i += 1) {
            if (input[i] != output.items[i]) {
                panic("Mismatch at byte {}: {c} vs {c}", .{ i, input[i], output.items[i] });
            }
        }
        expectEqual(input.len, output.items.len);
    }

    pub fn getTotalBytes(self: BufferTree) usize {
        var num_bytes: usize = 0;
        for (self.inner.root.state[0..self.inner.root.num_children]) |state| num_bytes += state.num_bytes;
        return num_bytes;
    }

    pub fn getTotalNewlines(self: BufferTree) usize {
        var num_newlines: usize = 0;
        for (self.inner.root.state[0..self.inner.root.num_children]) |state| num_newlines += state.num_newlines;
        return num_newlines;
    }
};

test "tree insert all at once" {
    const cm: []const u8 = (std.fs.cwd().openFile("/home/jamie/huge.js", .{}) catch unreachable).readToEndAlloc(std.testing.allocator, std.math.maxInt(usize)) catch unreachable;
    defer std.testing.allocator.free(cm);

    var tree = BufferTree.init(std.testing.allocator);
    defer tree.deinit();
    tree.insert(0, cm);
    tree.inner.validate();
    tree.testEqual(cm);
    expectEqual(tree.inner.getDepth(), 3);
}

test "tree insert forwards" {
    const cm: []const u8 = (std.fs.cwd().openFile("/home/jamie/huge.js", .{}) catch unreachable).readToEndAlloc(std.testing.allocator, std.math.maxInt(usize)) catch unreachable;
    defer std.testing.allocator.free(cm);

    var tree = BufferTree.init(std.testing.allocator);
    defer tree.deinit();
    var i: usize = 0;
    while (i < cm.len) : (i += 107) {
        tree.insert(i, cm[i..min(i + 107, cm.len)]);
    }
    tree.inner.validate();
    tree.testEqual(cm);
    expectEqual(tree.inner.getDepth(), 3);
}

test "tree insert backwards" {
    const cm: []const u8 = (std.fs.cwd().openFile("/home/jamie/huge.js", .{}) catch unreachable).readToEndAlloc(std.testing.allocator, std.math.maxInt(usize)) catch unreachable;
    defer std.testing.allocator.free(cm);

    var tree = BufferTree.init(std.testing.allocator);
    defer tree.deinit();
    var i: usize = 0;
    while (i < cm.len) : (i += 107) {
        tree.insert(0, cm[if (cm.len - i > 107) cm.len - i - 107 else 0 .. cm.len - i]);
    }
    tree.inner.validate();
    tree.testEqual(cm);
    expectEqual(tree.inner.getDepth(), 3);
}

test "tree delete all at once" {
    const cm: []const u8 = (std.fs.cwd().openFile("/home/jamie/huge.js", .{}) catch unreachable).readToEndAlloc(std.testing.allocator, std.math.maxInt(usize)) catch unreachable;
    defer std.testing.allocator.free(cm);

    var tree = BufferTree.init(std.testing.allocator);
    defer tree.deinit();
    tree.insert(0, cm);

    tree.delete(0, cm.len);
    tree.inner.validate();
    tree.testEqual("");
}

test "tree delete forwards" {
    const cm: []const u8 = (std.fs.cwd().openFile("/home/jamie/huge.js", .{}) catch unreachable).readToEndAlloc(std.testing.allocator, std.math.maxInt(usize)) catch unreachable;
    defer std.testing.allocator.free(cm);

    var tree = BufferTree.init(std.testing.allocator);
    defer tree.deinit();
    tree.insert(0, cm);

    const halfway = @divTrunc(cm.len, 2);
    var i: usize = 0;
    while (i < halfway) : (i += 107) {
        tree.delete(0, min(107, halfway - i));
    }
    tree.inner.validate();
    tree.testEqual(cm[halfway..]);
}

test "tree delete backwards" {
    const cm: []const u8 = (std.fs.cwd().openFile("/home/jamie/huge.js", .{}) catch unreachable).readToEndAlloc(std.testing.allocator, std.math.maxInt(usize)) catch unreachable;
    defer std.testing.allocator.free(cm);

    var tree = BufferTree.init(std.testing.allocator);
    defer tree.deinit();
    tree.insert(0, cm);

    const halfway = @divTrunc(cm.len, 2);
    var i: usize = 0;
    while (i < halfway) : (i += 107) {
        tree.delete(if (halfway - i > 107) halfway - i - 107 else 0, halfway - i);
    }
    tree.inner.validate();
    tree.testEqual(cm[halfway..]);
}

test "search forwards" {
    const cm: []const u8 = (std.fs.cwd().openFile("/home/jamie/huge.js", .{}) catch unreachable).readToEndAlloc(std.testing.allocator, std.math.maxInt(usize)) catch unreachable;
    defer std.testing.allocator.free(cm);

    var tree = BufferTree.init(std.testing.allocator);
    defer tree.deinit();
    tree.insert(0, cm);

    const needle = "className";

    var expected = ArrayList(usize).init(std.testing.allocator);
    defer expected.deinit();
    {
        var start: usize = 0;
        while (std.mem.indexOfPos(u8, cm, start, needle)) |pos| {
            expected.append(pos) catch oom();
            start = pos + 1;
        }
    }

    var actual = ArrayList(usize).init(std.testing.allocator);
    defer actual.deinit();
    {
        var start: usize = 0;
        while (tree.searchForwards(start, needle)) |pos| {
            actual.append(pos) catch oom();
            start = pos + 1;
        }
    }

    assert(meta.deepEqual(expected.items, actual.items));
}

test "search backwards" {
    const cm: []const u8 = (std.fs.cwd().openFile("/home/jamie/huge.js", .{}) catch unreachable).readToEndAlloc(std.testing.allocator, std.math.maxInt(usize)) catch unreachable;
    defer std.testing.allocator.free(cm);

    var tree = BufferTree.init(std.testing.allocator);
    defer tree.deinit();
    tree.insert(0, cm);

    const needle = "className";

    var expected = ArrayList(usize).init(std.testing.allocator);
    defer expected.deinit();
    {
        var start: usize = 0;
        while (std.mem.indexOfPos(u8, cm, start, needle)) |pos| {
            expected.append(pos) catch oom();
            start = pos + 1;
        }
    }

    var actual = ArrayList(usize).init(std.testing.allocator);
    defer actual.deinit();
    {
        var start: usize = tree.getTotalBytes();
        while (tree.searchBackwards(start, needle)) |pos| {
            actual.append(pos) catch oom();
            start = pos + 1;
        }
    }

    assert(meta.deepEqual(expected.items, actual.items));
}

test "get line start" {
    const cm: []const u8 = (std.fs.cwd().openFile("/home/jamie/huge.js", .{}) catch unreachable).readToEndAlloc(std.testing.allocator, std.math.maxInt(usize)) catch unreachable;
    defer std.testing.allocator.free(cm);

    var tree = BufferTree.init(std.testing.allocator);
    defer tree.deinit();
    tree.insert(0, cm);

    var expected = ArrayList(usize).init(std.testing.allocator);
    defer expected.deinit();
    {
        var start: usize = 0;
        expected.append(0) catch oom();
        while (std.mem.indexOfScalarPos(u8, cm, start, '\n')) |pos| {
            expected.append(pos + 1) catch oom();
            start = pos + 1;
        }
    }

    for (expected.items) |pos, line| {
        const point = tree.getPointForLineStart(line).?;
        expectEqual(pos, point.pos);
        expectEqual(line, BufferTree.getLine(point));
    }

    expectEqual(tree.getPointForLineStart(expected.items.len), null);
}

test "get awkward line start" {
    var tree = BufferTree.init(std.testing.allocator);
    defer tree.deinit();
    {
        var i: usize = 0;
        while (i < @divTrunc(BufferTreeInner.config.items_per_leaf, 2) - 1) : (i += 1) {
            tree.insert(i, " ");
        }
        tree.insert(i, "\n");
    }

    {
        expectEqual(tree.getPointForLineStart(0).?.pos, 0);
        const point = tree.getPointForLineStart(1).?;
        // point is at end of leaf
        expectEqual(point.pos, tree.getTotalBytes());
        expectEqual(point.offset, @intCast(u16, tree.getTotalBytes()));
    }

    // split branch
    while (tree.inner.root.num_children == 1) {
        tree.insert(tree.getTotalBytes(), " ");
    }
    // newline is at end of leaf
    const leaf = tree.inner.root.children[0].asLeaf();
    expectEqual(
        leaf.items[tree.inner.root.state[0].num_bytes - 1],
        '\n',
    );

    {
        expectEqual(tree.getPointForLineStart(0).?.pos, 0);
        const point = tree.getPointForLineStart(1).?;
        // point is at beginning of new leaf
        expectEqual(point.pos, @divTrunc(BufferTreeInner.config.items_per_leaf, 2));
        expectEqual(point.offset, 0);
    }
}
