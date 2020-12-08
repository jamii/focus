const focus = @import("../focus.zig");
usingnamespace focus.common;

// TODO size leaves and branches to be allocator friendly
// TODO try PackedIntArray

const page_size = 4 * 1024;

pub const Node = packed struct {
    parent: ?*Node,
    tag: packed enum(u8) { Leaf, Branch },

    fn getParent(self: Node) ?Branch {
        if (self.parent) |parent_node|
            return Branch.fromNode(parent_node)
        else
            return null;
    }

    pub fn findInParent(self: *Node) Branch.Offset {
        return self.getParent().?.findChild(self);
    }
};

pub const Leaf = struct {
    node: *Node,
    bytes: *[max_bytes]u8,

    pub const max_bytes = page_size - @sizeOf(Node);
    pub const Offset = u16;
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

    fn fromNode(node: *Node) Leaf {
        assert(node.tag == .Leaf);
        var address = @ptrToInt(node);
        address += @sizeOf(Node);
        const bytes = @intToPtr(*[max_bytes]u8, address);
        return .{ .node = node, .bytes = bytes };
    }
};

pub const Branch = struct {
    node: *Node,
    num_children: *Offset,
    children: *[max_children]*Node,
    num_bytes: *[max_children]usize,

    pub const max_children = @divTrunc(
        @sizeOf(usize) * @divTrunc(
            page_size - @sizeOf(Node) - @sizeOf(Offset),
            @sizeOf(usize),
        ),
        @sizeOf(*Node) + @sizeOf(*usize),
    );
    pub const Offset = u16;
    comptime {
        assert(std.math.maxInt(Offset) > max_children);
    }

    fn init(allocator: *Allocator) Branch {
        const page = allocator.alloc(u8, page_size) catch oom();
        const node = @ptrCast(*Node, page);
        node.parent = null;
        node.tag = .Branch;
        var self = fromNode(node);
        self.num_children.* = 0;
        return self;
    }

    fn fromNode(node: *Node) Branch {
        assert(node.tag == .Branch);
        var address = @ptrToInt(node);
        address += @sizeOf(Node);
        address += @sizeOf(Offset) - @mod(address, @sizeOf(Offset));
        const num_children = @intToPtr(*Offset, address);
        address += @sizeOf(Offset);
        dump(address);
        const children = @intToPtr(*[max_children]*Node, address);
        address += @sizeOf([max_children]*Node);
        const num_bytes = @intToPtr(*[max_children]usize, address);
        return .{ .node = node, .num_children = num_children, .children = children, .num_bytes = num_bytes };
    }

    pub fn findChild(self: Branch, child: *Node) Offset {
        return @intCast(Offset, std.mem.indexOfScalar(*Node, self.children[0..self.num_children.*], child).?);
    }

    pub fn insertChild(self: Branch, child_ix: usize, child: *Node, num_bytes: usize) void {
        assert(self.num_children.* < Branch.max_children);
        std.mem.copyBackwards(
            *Node,
            self.children[child_ix + 1 ..],
            self.children[child_ix..self.num_children.*],
        );
        self.children[child_ix] = child;
        self.num_bytes[child_ix] = num_bytes;
        self.num_children.* += 1;
        child.parent = self.node;
    }

    pub fn getNumBytes(self: Branch) usize {
        var num_bytes: usize = 0;
        for (self.num_bytes[0..self.num_children.*]) |n|
            num_bytes += n;
        return num_bytes;
    }
};

pub const Point = struct {
    leaf: Leaf,
    offset: Leaf.Offset,
};

pub const Tree = struct {
    allocator: *Allocator,
    root: Branch,

    pub fn init(allocator: *Allocator) Tree {
        var branch = Branch.init(allocator);
        var leaf = Leaf.init(allocator);
        branch.insertChild(0, leaf.node, 0);
        return .{
            .allocator = allocator,
            .root = branch,
        };
    }

    pub fn getPointForPos(self: *Tree, pos: usize, which: enum { Earliest, Latest }) ?Point {
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
                pos_remaining -= num_child_bytes;
                child_ix += 1;
                if (child_ix == num_children) {
                    node = branch.children[child_ix - 1];
                    continue :node;
                }
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

    pub fn insert(self: *Tree, start: usize, _bytes: []const u8) void {
        var arena = ArenaAllocator.init(self.allocator);
        defer arena.deinit();

        // find start point
        var point = self.getPointForPos(start, .Earliest).?;

        // make a stack of bytes to insert
        var bytes_stack = ArrayList([]const u8).initCapacity(&arena.allocator, 2) catch oom();
        bytes_stack.append(_bytes) catch oom();

        while (bytes_stack.popOrNull()) |bytes| {
            const leaf_child_ix = point.leaf.node.findInParent();
            const num_leaf_bytes = point.leaf.node.getParent().?.num_bytes[leaf_child_ix];

            if (bytes.len < point.leaf.bytes.len - num_leaf_bytes) {
                // insert in this leaf
                std.mem.copyBackwards(
                    u8,
                    point.leaf.bytes[point.offset + bytes.len .. num_leaf_bytes + bytes.len],
                    point.leaf.bytes[point.offset..num_leaf_bytes],
                );
                std.mem.copy(
                    u8,
                    point.leaf.bytes[point.offset .. point.offset + bytes.len],
                    bytes,
                );
                point.leaf.node.getParent().?.num_bytes[leaf_child_ix] = num_leaf_bytes + bytes.len;
            } else {
                // copy bytes after point onto the insert stack
                const new_bytes = std.mem.dupe(&arena.allocator, u8, point.leaf.bytes[point.offset..num_leaf_bytes]) catch oom();
                bytes_stack.append(new_bytes) catch oom();

                // insert what we can in this leaf
                const num_insert_bytes = min(point.leaf.bytes.len - point.offset, bytes.len);
                std.mem.copy(
                    u8,
                    point.leaf.bytes,
                    bytes[0..num_insert_bytes],
                );
                point.leaf.node.getParent().?.num_bytes[leaf_child_ix] = point.leaf.bytes.len;

                // push any remaining bytes back onto insert stack
                if (num_insert_bytes < bytes.len) {
                    bytes_stack.append(bytes[num_insert_bytes..]) catch oom();
                }

                // insert a new leaf somewhere and start there in the next loop iteration
                const new_leaf = self.insertLeaf(point);
                point = .{ .leaf = new_leaf, .offset = 0 };
            }
        }
    }

    fn insertLeaf(self: *Tree, point: Point) Leaf {
        const new_leaf = Leaf.init(self.allocator);
        var node = new_leaf.node;
        var num_bytes: usize = 0;
        var child_ix = point.leaf.node.findInParent();
        var branch = point.leaf.node.getParent().?;
        while (true) {
            if (branch.num_children.* < branch.children.len) {
                // insert node
                branch.insertChild(child_ix, node, num_bytes);
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
                new_branch.num_children.* = branch.num_children.* - split_point;
                branch.num_children.* = split_point;

                // insert node
                branch.insertChild(child_ix, node, num_bytes);

                // update branch parent
                if (branch.node.getParent()) |parent| {
                    parent.num_bytes[parent.findChild(branch.node)] = branch.getNumBytes();
                }

                if (branch.node.getParent()) |parent| {
                    // if have parent, insert new_branch into parent in next loop iteration
                    node = new_branch.node;
                    num_bytes = new_branch.getNumBytes();
                    child_ix = branch.node.findInParent();
                    branch = parent;
                    continue;
                } else {
                    // if no parent, make one and insert branch and new_branch
                    const new_parent = Branch.init(self.allocator);
                    self.root = new_parent;
                    new_parent.insertChild(0, branch.node, branch.getNumBytes());
                    new_parent.insertChild(1, new_branch.node, new_branch.getNumBytes());
                    break;
                }
            }
        }
        return new_leaf;
    }
};

test "tree insert" {
    // TODO deinit
    var arena = ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();

    var tree = Tree.init(&arena.allocator);
    tree.insert(0, "hello world");

    dump(tree);
}
