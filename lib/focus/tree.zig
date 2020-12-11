const focus = @import("../focus.zig");
usingnamespace focus.common;
const meta = focus.meta;

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

pub const Point = struct {
    // Always points at a byte, unless we're at the end of the tree
    pos: usize,
    leaf: Leaf,
    num_leaf_bytes: Leaf.Offset,
    offset: Leaf.Offset,

    fn isAtEnd(self: Point) bool {
        return self.offset == self.num_leaf_bytes;
    }

    fn getNextByte(self: *Point) u8 {
        assert(!self.isAtEnd());
        return self.leaf.bytes[self.offset];
    }

    const Seek = enum { Found, NotFound };

    fn seekNextLeaf(self: *Point) Seek {
        var node = self.leaf.node;

        self.pos += self.num_leaf_bytes - self.offset;

        // go up
        while (true) {
            if (node.getParent()) |parent| {
                const child_ix = parent.findChild(node);
                if (child_ix + 1 >= parent.num_children.*) {
                    // keep going up
                    node = parent.node;
                } else {
                    // go down
                    var child = parent.children[child_ix + 1];
                    var num_bytes = parent.num_bytes[child_ix + 1];
                    while (child.tag == .Branch) {
                        const branch = Branch.fromNode(child);
                        child = branch.children[0];
                        num_bytes = branch.num_bytes[0];
                    }
                    self.leaf = Leaf.fromNode(child);
                    self.num_leaf_bytes = @intCast(Leaf.Offset, num_bytes);
                    self.offset = 0;
                    return .Found;
                }
            } else {
                return .NotFound;
            }
        }
    }

    fn seekNextByte(self: *Point) Seek {
        if (self.offset >= self.num_leaf_bytes - 1) {
            if (self.seekNextLeaf() == .NotFound) return .NotFound;
        } else {
            self.pos += 1;
            self.offset += 1;
        }
        return .Found;
    }

    fn seekPrevLeaf(self: *Point) Seek {
        var node = self.leaf.node;

        self.pos -= self.offset;

        // go up
        while (true) {
            if (node.getParent()) |parent| {
                const child_ix = parent.findChild(node);
                if (child_ix == 0) {
                    // keep going up
                    node = parent.node;
                } else {
                    // go down
                    var child = parent.children[child_ix - 1];
                    var num_bytes = parent.num_bytes[child_ix - 1];
                    while (child.tag == .Branch) {
                        const branch = Branch.fromNode(child);
                        child = branch.children[branch.num_children.* - 1];
                        num_bytes = branch.num_bytes[branch.num_children.* - 1];
                    }
                    self.leaf = Leaf.fromNode(child);
                    self.num_leaf_bytes = @intCast(Leaf.Offset, num_bytes);
                    self.offset = @intCast(Leaf.Offset, num_bytes) - 1;
                    return .Found;
                }
            } else {
                return .NotFound;
            }
        }
    }

    fn seekPrevByte(self: *Point) Seek {
        if (self.offset == 0) {
            if (self.seekPrevLeaf() == .NotFound) return .NotFound;
        } else {
            self.pos -= 1;
            self.offset -= 1;
        }
        return .Found;
    }
};

pub const Tree = struct {
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

    pub fn getPointForPos(self: Tree, pos: usize) ?Point {
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
                if (pos_remaining < num_child_bytes) {
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
                .pos = pos,
                .leaf = Leaf.fromNode(node),
                .num_leaf_bytes = @intCast(Leaf.Offset, node.getNumBytesFromParent()),
                .offset = @intCast(Leaf.Offset, pos_remaining),
            };
    }

    pub fn insert(self: *Tree, start: usize, _bytes: []const u8) void {
        // find start point
        var point = self.getPointForPos(start).?;

        var bytes = _bytes;
        while (bytes.len > 0) {
            // insert what we can here
            const num_insert_bytes = min(Leaf.max_bytes - point.num_leaf_bytes, bytes.len);
            std.mem.copyBackwards(
                u8,
                point.leaf.bytes[point.offset + num_insert_bytes .. point.num_leaf_bytes + num_insert_bytes],
                point.leaf.bytes[point.offset..point.num_leaf_bytes],
            );
            std.mem.copy(
                u8,
                point.leaf.bytes[point.offset .. point.offset + num_insert_bytes],
                bytes[0..num_insert_bytes],
            );
            point.offset += @intCast(Leaf.Offset, num_insert_bytes);
            point.num_leaf_bytes += @intCast(Leaf.Offset, num_insert_bytes);

            // save remaining bytes for next loop iter
            bytes = bytes[num_insert_bytes..];

            if (bytes.len == 0) {
                point.leaf.updateNumBytes(point.num_leaf_bytes);
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
                    point.num_leaf_bytes = Leaf.max_bytes - halfway;
                    point.offset -= halfway;
                } else {
                    point.num_leaf_bytes = halfway;
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

    pub fn delete(self: *Tree, start: usize, _end: usize) void {
        var end = _end;

        var total_bytes = self.root.sumNumBytes();
        assert(start <= end);
        assert(end <= total_bytes);

        while (start < end) {
            // find start point
            var point = self.getPointForPos(start).?;

            // delete what we can here
            var num_delete_bytes = min(end - start, point.num_leaf_bytes - point.offset);
            std.mem.copy(
                u8,
                point.leaf.bytes[point.offset..],
                point.leaf.bytes[point.offset + num_delete_bytes .. point.num_leaf_bytes],
            );

            end -= num_delete_bytes;
            point.num_leaf_bytes -= num_delete_bytes;
            total_bytes -= num_delete_bytes;

            if (point.num_leaf_bytes >= @divTrunc(Leaf.max_bytes, 2) or total_bytes < @divTrunc(Leaf.max_bytes, 2)) {
                point.leaf.updateNumBytes(point.num_leaf_bytes);
            } else {
                // leaf is underfull, remove it and insert bytes into sibling
                var removed = ArrayList(*Node).initCapacity(self.allocator, 16) catch oom();
                const leaf_child_ix = point.leaf.node.findInParent();
                point.leaf.node.getParent().?.removeChild(leaf_child_ix, &removed);
                self.insert(start - point.offset, point.leaf.bytes[0..point.num_leaf_bytes]);
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

    pub fn searchForwards(self: Tree, start: usize, needle: []const u8) ?usize {
        assert(needle.len > 0);
        var start_point = self.getPointForPos(start).?;
        if (start_point.isAtEnd()) return null;
        const needle_start_char = needle[0];
        while (true) {
            const haystack_start_char = start_point.getNextByte();
            if (haystack_start_char == needle_start_char) {
                var end_point = start_point;
                var is_match = true;
                for (needle[1..]) |needle_char| {
                    if (end_point.seekNextByte() == .Found)
                        if (end_point.getNextByte() == needle_char)
                            continue;
                    is_match = false;
                    break;
                }
                if (is_match) return start_point.pos;
            }
            if (start_point.seekNextByte() == .NotFound) return null;
        }
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

    pub fn copy(self: Tree, allocator: *Allocator, start: usize, end: usize) []const u8 {
        var buffer = allocator.alloc(u8, end - start) catch oom();
        self.copyInto(buffer, start);
        return buffer;
    }

    pub fn copyInto(self: Tree, buffer: []u8, start: usize) void {
        var point = self.getPointForPos(start).?;

        while (true) {
            const num_copy_bytes = min(buffer.len, point.num_leaf_bytes - point.offset);
            std.mem.copy(
                u8,
                buffer,
                point.leaf.bytes[point.offset .. point.offset + num_copy_bytes],
            );
            buffer = buffer[num_copy_bytes..];

            if (buffer.len == 0) break;

            assert(point.seekNextLeaf() != .NotFound);
        }
    }

    fn writeInto(self: Tree, writer: anytype, start: usize, end: usize) !void {
        var point = self.getPointForPos(start).?;

        var num_remaining_write_bytes = end - start;
        while (true) {
            const num_write_bytes = min(num_remaining_write_bytes, point.num_leaf_bytes - point.offset);
            try writer.writeAll(point.leaf.bytes[point.offset .. point.offset + num_write_bytes]);
            num_remaining_write_bytes -= num_write_bytes;

            if (num_remaining_write_bytes == 0) break;

            assert(point.seekNextLeaf() != .NotFound);
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
    tree.writeInto(output.writer(), 0, tree.getTotalBytes()) catch unreachable;
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

test "search forwards" {
    const cm: []const u8 = (std.fs.cwd().openFile("/home/jamie/huge.js", .{}) catch unreachable).readToEndAlloc(std.testing.allocator, std.math.maxInt(usize)) catch unreachable;
    defer std.testing.allocator.free(cm);

    var tree = Tree.init(std.testing.allocator);
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
