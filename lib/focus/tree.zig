const focus = @import("../focus.zig");
usingnamespace focus.common;

// TODO how unbalanced can this get?
// TODO try PackedIntArray for Offset etc
// TODO Leaf/Branch can be done much nicer with packed structs once they are less buggy

const page_size = 4 * 1024;

fn roundDownTo(i: usize, mod: usize) usize {
    return mod * @divTrunc(i, mod);
}

fn roundUpTo(i: usize, mod: usize) usize {
    return mod * (@divTrunc(i, mod) + if (@mod(i, mod) > 0) @intCast(usize, 1) else 0);
}

fn alignTo(comptime T: type, address: usize) usize {
    return roundUpTo(address, @alignOf(T));
}

const Node = packed struct {
    parent: ?*Node,
    tag: packed enum(u8) { Leaf, Branch },

    fn deinit(self: *Node, allocator: *Allocator) void {
        switch (self.tag) {
            .Leaf => Leaf.fromNode(self).deinit(allocator),
            .Branch => Branch.fromNode(self).deinit(allocator),
        }
    }

    fn getParent(self: Node) ?Branch {
        if (self.parent) |parent_node|
            return Branch.fromNode(parent_node)
        else
            return null;
    }

    fn getNumBytesFromParent(self: *Node) usize {
        const parent = self.getParent().?;
        const child_ix = parent.findChild(self);
        return parent.num_bytes[child_ix];
    }

    fn findInParent(self: *Node) Branch.Offset {
        return self.getParent().?.findChild(self);
    }

    fn debugInto(self: *Node, output: *ArrayList(u8), indent: usize) void {
        switch (self.tag) {
            .Leaf => Leaf.fromNode(self).debugInto(output, indent),
            .Branch => Branch.fromNode(self).debugInto(output, indent),
        }
    }

    fn validate(self: *Node) void {
        switch (self.tag) {
            .Leaf => Leaf.fromNode(self).validate(),
            .Branch => Branch.fromNode(self).validate(false),
        }
    }
};

const Leaf = struct {
    node: *Node,
    bytes: *[max_bytes]u8,

    const max_bytes = page_size - @sizeOf(Node);
    const Offset = u16;
    comptime {
        assert(std.math.maxInt(Offset) > max_bytes);
    }

    fn init(allocator: *Allocator) Leaf {
        const page = allocator.alloc(u8, page_size) catch oom();
        const node = @ptrCast(*Node, page);
        node.parent = null;
        node.tag = .Leaf;
        return fromNode(node);
    }

    fn deinit(
        self: Leaf,
        allocator: *Allocator,
    ) void {
        const page = @ptrCast(*[page_size]u8, self.node);
        allocator.free(page);
    }

    const bytes_offset = @sizeOf(Node);

    fn fromNode(node: *Node) Leaf {
        assert(node.tag == .Leaf);
        var address = @ptrToInt(node);
        return .{
            .node = node,
            .bytes = @intToPtr(*[max_bytes]u8, address + bytes_offset),
        };
    }

    fn updateNumBytes(self: Leaf, num_bytes: usize) void {
        const parent = self.node.getParent().?;
        const child_ix = parent.findChild(self.node);
        parent.num_bytes[child_ix] = num_bytes;
        parent.updateSpine();
    }

    fn nextLeaf(self: Leaf) ?Leaf {
        var node = self.node;

        // go up
        while (true) {
            if (node.getParent()) |parent| {
                const child_ix = parent.findChild(node);
                if (child_ix + 1 < parent.num_children.*) {
                    // go down
                    var child = parent.children[child_ix + 1];
                    while (child.tag == .Branch) {
                        child = Branch.fromNode(child).children[0];
                    }
                    return Leaf.fromNode(child);
                } else {
                    node = parent.node;
                }
            } else {
                return null;
            }
        }
    }

    fn prevLeaf(self: Leaf) ?Leaf {
        var node = self.node;

        // go up
        while (true) {
            if (node.getParent()) |parent| {
                const child_ix = parent.findChild(node);
                if (child_ix > 0) {
                    // go down
                    var child = parent.children[child_ix - 1];
                    while (child.tag == .Branch) {
                        const branch = Branch.fromNode(node);
                        node = branch.children[branch.num_children.* - 1];
                    }
                    return Leaf.fromNode(child);
                } else {
                    node = parent.node;
                }
            } else {
                return null;
            }
        }
    }

    fn debugInto(self: Leaf, output: *ArrayList(u8), indent: usize) void {}

    fn validate(self: Leaf) void {}
};

const Branch = struct {
    node: *Node,
    num_children: *Offset,
    children: *[max_children]*Node,
    num_bytes: *[max_children]usize,

    const max_children = @divTrunc(
        @sizeOf(usize) * @divTrunc(
            page_size - @sizeOf(Node) - @sizeOf(Offset),
            @sizeOf(usize),
        ),
        @sizeOf(*Node) + @sizeOf(*usize),
    );
    const Offset = u16;
    comptime {
        assert(std.math.maxInt(Offset) > max_children);
    }

    fn init(allocator: *Allocator) Branch {
        const page = allocator.allocWithOptions(u8, page_size, @alignOf(usize), null) catch oom();
        const node = @ptrCast(*Node, page);
        node.parent = null;
        node.tag = .Branch;
        var self = fromNode(node);
        self.num_children.* = 0;
        return self;
    }

    fn deinit(
        self: Branch,
        allocator: *Allocator,
    ) void {
        var child_ix: usize = 0;
        while (child_ix < self.num_children.*) : (child_ix += 1) {
            self.children[child_ix].deinit(allocator);
        }
        const page = @ptrCast(*[page_size]u8, self.node);
        allocator.free(page);
    }

    const num_children_offset = alignTo(Offset, @sizeOf(Node));
    const children_offset = alignTo(*Node, num_children_offset + @sizeOf(Offset));
    const num_bytes_offset = alignTo(usize, children_offset + @sizeOf([max_children]*Node));

    fn fromNode(node: *Node) Branch {
        assert(node.tag == .Branch);
        var address = @ptrToInt(node);
        return .{
            .node = node,
            .num_children = @intToPtr(*Offset, address + num_children_offset),
            .children = @intToPtr(*[max_children]*Node, address + children_offset),
            .num_bytes = @intToPtr(*[max_children]usize, address + num_bytes_offset),
        };
    }

    fn findChild(self: Branch, child: *Node) Offset {
        return @intCast(Offset, std.mem.indexOfScalar(*Node, self.children[0..self.num_children.*], child).?);
    }

    fn insertChild(self: Branch, child_ix: usize, child: *Node, num_bytes: usize) void {
        assert(self.num_children.* < Branch.max_children);
        std.mem.copyBackwards(
            *Node,
            self.children[child_ix + 1 ..],
            self.children[child_ix..self.num_children.*],
        );
        std.mem.copyBackwards(
            usize,
            self.num_bytes[child_ix + 1 ..],
            self.num_bytes[child_ix..self.num_children.*],
        );
        self.children[child_ix] = child;
        self.num_bytes[child_ix] = num_bytes;
        self.num_children.* += 1;
        child.parent = self.node;
        self.updateSpine();
    }

    fn removeChild(self: Branch, _child_ix: usize, removed: *ArrayList(*Node)) void {
        var branch = self;
        var child_ix = _child_ix;
        while (true) {
            assert(child_ix < branch.num_children.*);
            removed.append(branch.children[child_ix]) catch oom();
            std.mem.copy(
                *Node,
                branch.children[child_ix..],
                branch.children[child_ix + 1 .. branch.num_children.*],
            );
            std.mem.copy(
                usize,
                branch.num_bytes[child_ix..],
                branch.num_bytes[child_ix + 1 .. branch.num_children.*],
            );
            branch.num_children.* -= 1;
            if (branch.num_children.* == 0) {
                // if getParent is null, then we just deleted the last leaf node, which shouldn't happen
                const parent = branch.node.getParent().?;
                child_ix = parent.findChild(branch.node);
                branch = parent;
            } else {
                branch.updateSpine();
                break;
            }
        }
    }

    fn updateSpine(self: Branch) void {
        var branch = self;
        while (branch.node.getParent()) |parent| {
            const child_ix = parent.findChild(branch.node);
            parent.num_bytes[child_ix] = branch.sumNumBytes();
            branch = parent;
        }
    }

    fn sumNumBytes(self: Branch) usize {
        var num_bytes: usize = 0;
        for (self.num_bytes[0..self.num_children.*]) |n|
            num_bytes += n;
        return num_bytes;
    }

    fn debugInto(self: Branch, output: *ArrayList(u8), indent: usize) void {
        output.append('\n') catch oom();
        output.appendNTimes(' ', indent) catch oom();
        std.fmt.format(output.outStream(), "* num_children={} num_bytes={}=[", .{ self.num_children.*, self.sumNumBytes() }) catch oom();
        for (self.num_bytes[0..self.num_children.*]) |n, i| {
            const sep: []const u8 = if (i == 0) "" else ", ";
            std.fmt.format(output.outStream(), "{}{}", .{ sep, n }) catch oom();
        }
        std.fmt.format(output.outStream(), "]", .{}) catch oom();
        for (self.children[0..self.num_children.*]) |child| {
            child.debugInto(output, indent + 4);
        }
    }

    fn validate(self: Branch, is_root: bool) void {
        if (self.node.getParent()) |parent| {
            const child_ix = parent.findChild(self.node);
            assert(self.sumNumBytes() == parent.num_bytes[child_ix]);
        }
        // TODO rebalance underfull branches
        //if (!is_root) {
        //assert(self.num_children.* >= @divTrunc(Branch.max_children, 2));
        //}
        var child_ix: usize = 0;
        while (child_ix < self.num_children.*) : (child_ix += 1) {
            assert(self.children[child_ix].parent == self.node);
            assert(self.num_bytes[child_ix] >= @divTrunc(Leaf.max_bytes, 2));
            self.children[child_ix].validate();
        }
    }
};

const Point = struct {
    leaf: Leaf,
    offset: Leaf.Offset,
};

const Tree = struct {
    allocator: *Allocator,
    root: Branch,

    fn init(allocator: *Allocator) Tree {
        var branch = Branch.init(allocator);
        var leaf = Leaf.init(allocator);
        branch.insertChild(0, leaf.node, 0);
        return .{
            .allocator = allocator,
            .root = branch,
        };
    }

    fn deinit(self: Tree) void {
        self.root.deinit(self.allocator);
    }

    fn getPointForPos(self: Tree, pos: usize, which: enum { Earliest, Latest }) ?Point {
        var node = self.root.node;
        var pos_remaining = pos;
        var num_child_bytes: usize = undefined;
        node: while (node.tag == .Branch) {
            const branch = Branch.fromNode(node);
            const num_children = branch.num_children.*;
            const num_bytes = branch.num_bytes;
            var child_ix: usize = 0;
            while (true) {
                num_child_bytes = num_bytes[child_ix];
                if (pos_remaining < num_child_bytes or (which == .Earliest and pos_remaining == num_child_bytes)) {
                    node = branch.children[child_ix];
                    continue :node;
                }
                child_ix += 1;
                if (child_ix == num_children) {
                    node = branch.children[child_ix - 1];
                    continue :node;
                }
                pos_remaining -= num_child_bytes;
            }
        }
        return if (pos_remaining > num_child_bytes)
            null
        else
            .{
                .leaf = Leaf.fromNode(node),
                .offset = @intCast(Leaf.Offset, pos_remaining),
            };
    }

    fn insert(self: *Tree, start: usize, _bytes: []const u8) void {
        // find start point
        var point = self.getPointForPos(start, .Earliest).?;

        var bytes = _bytes;
        while (bytes.len > 0) {
            const leaf_child_ix = point.leaf.node.findInParent();
            const num_leaf_bytes = point.leaf.node.getNumBytesFromParent();

            // insert what we can here
            const num_insert_bytes = min(Leaf.max_bytes - num_leaf_bytes, bytes.len);
            std.mem.copyBackwards(
                u8,
                point.leaf.bytes[point.offset + num_insert_bytes .. num_leaf_bytes + num_insert_bytes],
                point.leaf.bytes[point.offset..num_leaf_bytes],
            );
            std.mem.copy(
                u8,
                point.leaf.bytes[point.offset .. point.offset + num_insert_bytes],
                bytes[0..num_insert_bytes],
            );
            point.offset += @intCast(Leaf.Offset, num_insert_bytes);

            // save remaining bytes for next loop iter
            bytes = bytes[num_insert_bytes..];

            if (bytes.len == 0) {
                point.leaf.updateNumBytes(num_leaf_bytes + num_insert_bytes);
                break;
            } else {
                // split leaf
                const halfway = @divTrunc(Leaf.max_bytes, 2);
                const new_leaf = self.insertLeafAfter(point.leaf);
                std.mem.copy(
                    u8,
                    new_leaf.bytes,
                    point.leaf.bytes[halfway..],
                );
                point.leaf.updateNumBytes(halfway);
                new_leaf.updateNumBytes(Leaf.max_bytes - halfway);

                // adjust point
                if (point.offset >= halfway) {
                    point.leaf = new_leaf;
                    point.offset -= halfway;
                }
            }
        }
    }

    fn insertLeafAfter(self: *Tree, after: Leaf) Leaf {
        const new_leaf = Leaf.init(self.allocator);
        var node = new_leaf.node;
        var num_bytes: usize = 0;
        var child_ix = after.node.findInParent();
        var branch = after.node.getParent().?;
        while (true) {
            if (branch.num_children.* < branch.children.len) {
                // insert node
                branch.insertChild(child_ix + 1, node, num_bytes);
                break;
            } else {
                // split off a new branch
                const new_branch = Branch.init(self.allocator);
                const split_point = @divTrunc(Branch.max_children, 2);
                std.mem.copy(
                    *Node,
                    new_branch.children,
                    branch.children[split_point..],
                );
                std.mem.copy(
                    usize,
                    new_branch.num_bytes,
                    branch.num_bytes[split_point..],
                );
                new_branch.num_children.* = branch.num_children.* - split_point;
                branch.num_children.* = split_point;
                for (new_branch.children[0..new_branch.num_children.*]) |child| {
                    child.parent = new_branch.node;
                }
                branch.updateSpine();

                // insert node
                if (child_ix < split_point)
                    branch.insertChild(child_ix + 1, node, num_bytes)
                else
                    new_branch.insertChild(child_ix - split_point + 1, node, num_bytes);

                if (branch.node.getParent()) |parent| {
                    // if have parent, insert new_branch into parent in next loop iteration
                    node = new_branch.node;
                    num_bytes = new_branch.sumNumBytes();
                    child_ix = branch.node.findInParent();
                    branch = parent;
                    continue;
                } else {
                    // if no parent, make one and insert branch and new_branch
                    const new_parent = Branch.init(self.allocator);
                    self.root = new_parent;
                    new_parent.insertChild(0, branch.node, branch.sumNumBytes());
                    new_parent.insertChild(1, new_branch.node, new_branch.sumNumBytes());
                    break;
                }
            }
        }
        return new_leaf;
    }

    fn delete(self: *Tree, start: usize, _end: usize) void {
        var end = _end;

        var total_bytes = self.root.sumNumBytes();
        assert(start <= end);
        assert(end <= total_bytes);

        while (start < end) {

            // find start point
            const point = self.getPointForPos(start, .Latest).?;
            const leaf_child_ix = point.leaf.node.findInParent();
            var num_leaf_bytes = point.leaf.node.getNumBytesFromParent();

            // delete what we can here
            var num_delete_bytes = min(end - start, num_leaf_bytes - point.offset);
            std.mem.copy(
                u8,
                point.leaf.bytes[point.offset..],
                point.leaf.bytes[point.offset + num_delete_bytes .. num_leaf_bytes],
            );

            end -= num_delete_bytes;
            num_leaf_bytes -= num_delete_bytes;
            total_bytes -= num_delete_bytes;

            if (num_leaf_bytes >= @divTrunc(Leaf.max_bytes, 2) or total_bytes < @divTrunc(Leaf.max_bytes, 2)) {
                point.leaf.updateNumBytes(num_leaf_bytes);
            } else {
                // leaf is underfull, remove it and insert bytes into sibling
                var removed = ArrayList(*Node).initCapacity(self.allocator, 16) catch oom();
                point.leaf.node.getParent().?.removeChild(leaf_child_ix, &removed);
                self.insert(start - point.offset, point.leaf.bytes[0..num_leaf_bytes]);
                for (removed.items) |node| node.deinit(self.allocator);
                removed.deinit();
            }
        }
    }

    fn startLeaf(self: Tree) Leaf {
        var node = self.root.node;
        while (node.tag == .Branch) {
            node = Branch.fromNode(node).children[0];
        }
        return Leaf.fromNode(node);
    }

    fn endLeaf(self: Tree) Leaf {
        var node = self.root.node;
        while (node.tag == .Branch) {
            const branch = Branch.fromNode(node);
            node = branch.children[branch.num_children.* - 1];
        }
        return Leaf.fromNode(node);
    }

    fn getTotalBytes(self: Tree) usize {
        return self.root.sumNumBytes();
    }

    fn getDepth(self: Tree) usize {
        var depth: usize = 0;
        var node = self.root.node;
        while (node.tag == .Branch) {
            depth += 1;
            node = Branch.fromNode(node).children[0];
        }
        return depth;
    }

    fn copyInto(self: Tree, output: *ArrayList(u8), _start: usize, end: usize) void {
        var start = _start;
        var point = self.getPointForPos(start, .Latest).?;

        while (true) {
            const num_leaf_bytes = point.leaf.node.getNumBytesFromParent();
            const num_copy_bytes = min(end - start, num_leaf_bytes - point.offset);
            output.appendSlice(point.leaf.bytes[point.offset..num_leaf_bytes]) catch oom();
            start += num_leaf_bytes;

            if (start >= end) break;

            point.leaf = point.leaf.nextLeaf().?;
            point.offset = 0;
        }
    }

    fn debugInto(self: Tree, output: *ArrayList(u8)) void {
        self.root.debugInto(output, 0);
    }

    fn validate(self: Tree) void {
        const total_bytes = self.root.sumNumBytes();
        if (total_bytes < @divTrunc(Leaf.max_bytes, 2)) {
            var branch = self.root;
            while (true) {
                assert(branch.num_children.* == 1);
                const child = branch.children[0];
                switch (child.tag) {
                    .Leaf => break,
                    .Branch => branch = Branch.fromNode(child),
                }
            }
        } else {
            self.root.validate(true);
        }
    }
};

fn testEqual(tree: *const Tree, input: []const u8) void {
    var output = ArrayList(u8).initCapacity(std.testing.allocator, input.len) catch oom();
    defer output.deinit();
    tree.copyInto(&output, 0, tree.getTotalBytes());
    var i: usize = 0;
    while (i < min(input.len, output.items.len)) : (i += 1) {
        if (input[i] != output.items[i]) {
            panic("Mismatch at byte {}: {c} vs {c}", .{ i, input[i], output.items[i] });
        }
    }
    expectEqual(input.len, output.items.len);
}

test "tree insert all at once" {
    const cm: []const u8 = (std.fs.cwd().openFile("/home/jamie/huge.js", .{}) catch unreachable).readToEndAlloc(std.testing.allocator, std.math.maxInt(usize)) catch unreachable;
    defer std.testing.allocator.free(cm);

    var tree = Tree.init(std.testing.allocator);
    defer tree.deinit();
    tree.insert(0, cm);
    tree.validate();
    testEqual(&tree, cm);
    expectEqual(tree.getDepth(), 3);
}

test "tree insert forwards" {
    const cm: []const u8 = (std.fs.cwd().openFile("/home/jamie/huge.js", .{}) catch unreachable).readToEndAlloc(std.testing.allocator, std.math.maxInt(usize)) catch unreachable;
    defer std.testing.allocator.free(cm);

    var tree = Tree.init(std.testing.allocator);
    defer tree.deinit();
    var i: usize = 0;
    while (i < cm.len) : (i += 107) {
        tree.insert(i, cm[i..min(i + 107, cm.len)]);
    }
    tree.validate();
    testEqual(&tree, cm);
    expectEqual(tree.getDepth(), 3);
}

test "tree insert backwards" {
    const cm: []const u8 = (std.fs.cwd().openFile("/home/jamie/huge.js", .{}) catch unreachable).readToEndAlloc(std.testing.allocator, std.math.maxInt(usize)) catch unreachable;
    defer std.testing.allocator.free(cm);

    var tree = Tree.init(std.testing.allocator);
    defer tree.deinit();
    var i: usize = 0;
    while (i < cm.len) : (i += 107) {
        tree.insert(0, cm[if (cm.len - i > 107) cm.len - i - 107 else 0 .. cm.len - i]);
    }
    tree.validate();
    testEqual(&tree, cm);
    expectEqual(tree.getDepth(), 3);
}

test "tree delete all at once" {
    const cm: []const u8 = (std.fs.cwd().openFile("/home/jamie/huge.js", .{}) catch unreachable).readToEndAlloc(std.testing.allocator, std.math.maxInt(usize)) catch unreachable;
    defer std.testing.allocator.free(cm);

    var tree = Tree.init(std.testing.allocator);
    defer tree.deinit();
    tree.insert(0, cm);

    tree.delete(0, cm.len);
    tree.validate();
    testEqual(&tree, "");
}

test "tree delete forwards" {
    const cm: []const u8 = (std.fs.cwd().openFile("/home/jamie/huge.js", .{}) catch unreachable).readToEndAlloc(std.testing.allocator, std.math.maxInt(usize)) catch unreachable;
    defer std.testing.allocator.free(cm);

    var tree = Tree.init(std.testing.allocator);
    defer tree.deinit();
    tree.insert(0, cm);

    const halfway = @divTrunc(cm.len, 2);
    var i: usize = 0;
    while (i < halfway) : (i += 107) {
        tree.delete(0, min(107, halfway - i));
    }
    tree.validate();
    testEqual(&tree, cm[halfway..]);
}

test "tree delete backwards" {
    const cm: []const u8 = (std.fs.cwd().openFile("/home/jamie/huge.js", .{}) catch unreachable).readToEndAlloc(std.testing.allocator, std.math.maxInt(usize)) catch unreachable;
    defer std.testing.allocator.free(cm);

    var tree = Tree.init(std.testing.allocator);
    defer tree.deinit();
    tree.insert(0, cm);

    const halfway = @divTrunc(cm.len, 2);
    var i: usize = 0;
    while (i < halfway) : (i += 107) {
        tree.delete(if (halfway - i > 107) halfway - i - 107 else 0, halfway - i);
    }
    tree.validate();
    testEqual(&tree, cm[halfway..]);
}