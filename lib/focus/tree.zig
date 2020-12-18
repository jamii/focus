const focus = @import("../focus.zig");
usingnamespace focus.common;
const meta = focus.meta;

pub const TreeConfig = struct {
    children_per_branch: usize,
    BranchState: type,
    items_per_leaf: usize,
    LeafState: type,
    Item: type,
};

// TODO how unbalanced can this get?
// TODO Leaf/Branch can be done with packed structs once they are less buggy
pub fn Tree(comptime _config: TreeConfig) type {
    return struct {
        allocator: *Allocator,
        root: *Branch,

        const TreeSelf = @This();
        pub const config = _config;

        pub fn init(allocator: *Allocator) TreeSelf {
            var branch = Branch.init(allocator);
            var leaf = Leaf.init(allocator);
            branch.insertChild(0, &leaf.node);
            return .{
                .allocator = allocator,
                .root = branch,
            };
        }

        pub fn deinit(self: TreeSelf) void {
            self.root.deinit(self.allocator);
        }

        pub fn insert(self: *TreeSelf, point: *Point, _items: []const config.Item) void {
            var items = _items;
            while (items.len > 0) {
                // insert what we can here
                const num_insert_items = min(config.items_per_leaf - point.leaf.num_items, items.len);
                std.mem.copyBackwards(
                    config.Item,
                    point.leaf.items[point.offset + num_insert_items .. point.leaf.num_items + num_insert_items],
                    point.leaf.items[point.offset..point.leaf.num_items],
                );
                std.mem.copy(
                    config.Item,
                    point.leaf.items[point.offset .. point.offset + num_insert_items],
                    items[0..num_insert_items],
                );
                point.pos += num_insert_items;
                point.offset += @intCast(Leaf.Offset, num_insert_items);
                point.leaf.num_items += @intCast(Leaf.Offset, num_insert_items);

                // save remaining items for next loop iter
                items = items[num_insert_items..];

                if (items.len == 0) {
                    point.leaf.updateSpine();
                    break;
                } else {
                    // split leaf
                    const halfway = @divTrunc(config.items_per_leaf, 2);
                    const new_leaf = self.insertLeafAfter(point.leaf);
                    std.mem.copy(
                        config.Item,
                        new_leaf.items[0..],
                        point.leaf.items[halfway..],
                    );
                    point.leaf.num_items = halfway;
                    point.leaf.updateSpine();
                    new_leaf.num_items = config.items_per_leaf - halfway;
                    new_leaf.updateSpine();

                    // adjust point
                    if (point.offset >= halfway) {
                        point.leaf = new_leaf;
                        point.offset -= @intCast(Leaf.Offset, halfway);
                    }
                }
            }
        }

        fn insertLeafAfter(self: *TreeSelf, after: *Leaf) *Leaf {
            const new_leaf = Leaf.init(self.allocator);
            var node = &new_leaf.node;
            var child_ix = after.node.findInParent();
            var branch = after.node.getParent().?;
            while (true) {
                if (branch.num_children < branch.children.len) {
                    // insert node
                    branch.insertChild(child_ix + 1, node);
                    break;
                } else {
                    // split off a new branch
                    const new_branch = Branch.init(self.allocator);
                    const split_point = @divTrunc(config.children_per_branch, 2);
                    std.mem.copy(
                        *Node,
                        new_branch.children[0..],
                        branch.children[split_point..],
                    );
                    std.mem.copy(
                        config.BranchState,
                        new_branch.state[0..],
                        branch.state[split_point..],
                    );
                    new_branch.num_children = branch.num_children - @intCast(Branch.Offset, split_point);
                    branch.num_children = split_point;
                    for (new_branch.children[0..new_branch.num_children]) |child| {
                        child.parent = &new_branch.node;
                    }

                    // insert node
                    if (child_ix < split_point) {
                        // calls branch.updateSpine()
                        branch.insertChild(child_ix + 1, node);
                    } else {
                        branch.updateSpine();
                        new_branch.insertChild(child_ix - split_point + 1, node);
                    }

                    if (branch.node.getParent()) |parent| {
                        // if have parent, insert new_branch into parent in next loop iteration
                        node = &new_branch.node;
                        child_ix = branch.node.findInParent();
                        branch = parent;
                        continue;
                    } else {
                        // if no parent, make one and insert branch and new_branch
                        const new_parent = Branch.init(self.allocator);
                        self.root = new_parent;
                        new_parent.insertChild(0, &branch.node);
                        new_parent.insertChild(1, &new_branch.node);
                        break;
                    }
                }
            }
            return new_leaf;
        }

        pub fn delete(self: *TreeSelf, point: *Point, num_items: usize) void {
            var num_items_remaining = num_items;

            while (num_items_remaining > 0) {
                // can only reach this state if we delete past the end of the tree
                assert(point.leaf.num_items > 0);

                // delete what we can here
                var num_delete_items = min(num_items_remaining, point.leaf.num_items - point.offset);
                std.mem.copy(
                    config.Item,
                    point.leaf.items[point.offset..],
                    point.leaf.items[point.offset + num_delete_items .. point.leaf.num_items],
                );

                num_items_remaining -= num_delete_items;
                point.leaf.num_items -= num_delete_items;

                if (point.leaf.num_items >= @divTrunc(config.items_per_leaf, 2) or point.leaf.isOnlyLeaf()) {
                    point.leaf.updateSpine();
                } else {
                    // leaf is underfull, remove it and insert items into sibling
                    const leaf = point.leaf;
                    if (point.seekPrevLeaf() == .NotFound) {
                        _ = point.seekNextLeaf();
                        point.pos -= leaf.num_items;
                    }
                    var removed = ArrayList(*Node).initCapacity(self.allocator, 16) catch oom();
                    const leaf_child_ix = leaf.node.findInParent();
                    leaf.node.getParent().?.removeChild(leaf_child_ix, &removed);
                    self.insert(point, leaf.items[0..point.leaf.num_items]);
                    for (removed.items) |node| node.deinit(self.allocator);
                    removed.deinit();
                }
            }
        }

        fn getStartLeaf(self: TreeSelf) *Leaf {
            var node = &self.root.node;
            while (node.tag == .Branch) {
                node = node.asBranch().children[0];
            }
            return node.asLeaf();
        }

        fn getEndLeaf(self: TreeSelf) *Leaf {
            var node = &self.root.node;
            while (node.tag == .Branch) {
                const branch = node.asBranch();
                node = branch.children[branch.num_children - 1];
            }
            return node.asLeaf();
        }

        pub fn getDepth(self: TreeSelf) usize {
            var depth: usize = 0;
            var node = &self.root.node;
            while (node.tag == .Branch) {
                depth += 1;
                node = node.asBranch().children[0];
            }
            return depth;
        }

        pub fn copy(self: TreeSelf, allocator: *Allocator, start: usize, end: usize) []const config.Item {
            var buffer = allocator.alloc(config.Item, end - start) catch oom();
            self.copyInto(buffer, start);
            return buffer;
        }

        pub fn copyInto(self: TreeSelf, _buffer: []config.Item, start: usize) void {
            var buffer = _buffer;

            var point = self.getPointForPos(start).?;

            while (true) {
                const num_copy_items = min(buffer.len, point.leaf.num_items - point.offset);
                std.mem.copy(
                    config.Item,
                    buffer,
                    point.leaf.items[point.offset .. point.offset + num_copy_items],
                );
                buffer = buffer[num_copy_items..];

                if (buffer.len == 0) break;

                assert(point.seekNextLeaf() != .NotFound);
            }
        }

        fn debugInto(self: TreeSelf, output: *ArrayList(u8)) void {
            self.root.debugInto(output, 0);
        }

        pub fn validate(self: TreeSelf) void {
            const leaf = self.getStartLeaf();
            if (leaf.isOnlyLeaf()) {
                var branch = self.root;
                while (branch.children[0].tag == .Branch) {
                    assert(branch.children[0].parent == &branch.node);
                    branch = branch.children[0].asBranch();
                }
                assert(leaf.num_items < @divTrunc(config.items_per_leaf, 2));
            } else {
                self.root.validate(true);
            }
        }

        pub const Node = struct {
            parent: ?*Node,
            tag: enum(u8) { Leaf, Branch },

            fn deinit(self: *Node, allocator: *Allocator) void {
                switch (self.tag) {
                    .Leaf => self.asLeaf().deinit(allocator),
                    .Branch => self.asBranch().deinit(allocator),
                }
            }

            pub fn getParent(self: *Node) ?*Branch {
                if (self.parent) |parent_node|
                    return @fieldParentPtr(Branch, "node", parent_node)
                else
                    return null;
            }

            pub fn findInParent(self: *Node) Branch.Offset {
                return self.getParent().?.findChild(self);
            }

            pub fn asLeaf(self: *Node) *Leaf {
                assert(self.tag == .Leaf);
                return @fieldParentPtr(Leaf, "node", self);
            }

            pub fn asBranch(self: *Node) *Branch {
                assert(self.tag == .Branch);
                return @fieldParentPtr(Branch, "node", self);
            }

            fn debugInto(self: *Node, output: *ArrayList(u8), indent: usize) void {
                switch (self.tag) {
                    .Leaf => self.asLeaf().debugInto(output, indent),
                    .Branch => self.asBranch().debugInto(output, indent),
                }
            }

            fn validate(self: *Node) void {
                switch (self.tag) {
                    .Leaf => self.asLeaf().validate(),
                    .Branch => self.asBranch().validate(false),
                }
            }
        };

        pub const Leaf = struct {
            node: Node,
            num_items: Offset,
            items: [config.items_per_leaf](config.Item),
            state: config.LeafState,

            pub const Offset = offset: {
                for (.{ u8, u16, u32, u64 }) |PotentialOffset| {
                    if (std.math.maxInt(PotentialOffset) > config.items_per_leaf) {
                        break :offset PotentialOffset;
                    }
                }
                unreachable;
            };

            fn init(allocator: *Allocator) *Leaf {
                const self = allocator.create(Leaf) catch oom();
                self.node.parent = null;
                self.node.tag = .Leaf;
                self.num_items = 0;
                self.state = config.LeafState.init(allocator);
                return self;
            }

            fn deinit(self: *Leaf, allocator: *Allocator) void {
                self.state.deinit(allocator);
                allocator.destroy(self);
            }

            fn updateSpine(self: *Leaf) void {
                const parent = self.node.getParent().?;
                const child_ix = parent.findChild(&self.node);
                self.state.update(self);
                parent.state[child_ix].update(&self.node);
                parent.updateSpine();
            }

            fn isOnlyLeaf(self: *Leaf) bool {
                var node = &self.node;
                while (node.getParent()) |parent| {
                    if (parent.num_children != 1) return false;
                    node = &parent.node;
                }
                return true;
            }

            fn debugInto(self: *const Leaf, output: *ArrayList(u8), indent: usize) void {}

            fn validate(self: *const Leaf) void {
                assert(self.num_items >= @divTrunc(config.items_per_leaf, 2));
            }
        };

        pub const Branch = struct {
            node: Node,
            num_children: Offset,
            children: [config.children_per_branch]*Node,
            state: [config.children_per_branch](config.BranchState),

            pub const Offset = offset: {
                for (.{ u8, u16, u32, u64 }) |PotentialOffset| {
                    if (std.math.maxInt(PotentialOffset) > config.children_per_branch) {
                        break :offset PotentialOffset;
                    }
                }
                unreachable;
            };

            fn init(allocator: *Allocator) *Branch {
                const self = allocator.create(Branch) catch oom();
                self.node.parent = null;
                self.node.tag = .Branch;
                self.num_children = 0;
                return self;
            }

            fn deinit(
                self: *Branch,
                allocator: *Allocator,
            ) void {
                var child_ix: usize = 0;
                while (child_ix < self.num_children) : (child_ix += 1) {
                    self.children[child_ix].deinit(allocator);
                }
                allocator.destroy(self);
            }

            pub fn findChild(self: *Branch, child: *Node) Offset {
                return @intCast(Offset, std.mem.indexOfScalar(*Node, self.children[0..self.num_children], child).?);
            }

            fn insertChild(self: *Branch, child_ix: usize, child: *Node) void {
                assert(self.num_children < config.children_per_branch);
                std.mem.copyBackwards(
                    *Node,
                    self.children[child_ix + 1 ..],
                    self.children[child_ix..self.num_children],
                );
                std.mem.copyBackwards(
                    config.BranchState,
                    self.state[child_ix + 1 ..],
                    self.state[child_ix..self.num_children],
                );
                self.children[child_ix] = child;
                self.state[child_ix] = config.BranchState.init();
                self.state[child_ix].update(child);
                self.num_children += 1;
                child.parent = &self.node;
                self.updateSpine();
            }

            fn removeChild(self: *Branch, _child_ix: usize, removed: *ArrayList(*Node)) void {
                var branch = self;
                var child_ix = _child_ix;
                while (true) {
                    assert(child_ix < branch.num_children);
                    removed.append(branch.children[child_ix]) catch oom();
                    std.mem.copy(
                        *Node,
                        branch.children[child_ix..],
                        branch.children[child_ix + 1 .. branch.num_children],
                    );
                    std.mem.copy(
                        config.BranchState,
                        branch.state[child_ix..],
                        branch.state[child_ix + 1 .. branch.num_children],
                    );
                    branch.num_children -= 1;
                    if (branch.num_children == 0) {
                        // if getParent is null, then we just deleted the last leaf node, which shouldn't happen
                        const parent = branch.node.getParent().?;
                        child_ix = parent.findChild(&branch.node);
                        branch = parent;
                    } else {
                        branch.updateSpine();
                        break;
                    }
                }
            }

            fn updateSpine(self: *Branch) void {
                var branch = self;
                while (branch.node.getParent()) |parent| {
                    const child_ix = parent.findChild(&branch.node);
                    parent.state[child_ix].update(&branch.node);
                    branch = parent;
                }
            }

            fn debugInto(self: *const Branch, output: *ArrayList(u8), indent: usize) void {
                output.append('\n') catch oom();
                output.appendNTimes(' ', indent) catch oom();
                std.fmt.format(output.outStream(), "* num_children={} [", .{self.num_children}) catch oom();
                for (self.num_items[0..self.num_children]) |n, i| {
                    const sep: []const u8 = if (i == 0) "" else ", ";
                    std.fmt.format(output.outStream(), "{}{}/{}", .{ sep, n, self.state[i] }) catch oom();
                }
                std.fmt.format(output.outStream(), "]", .{}) catch oom();
                for (self.children[0..self.num_children]) |child| {
                    child.debugInto(output, indent + 4);
                }
            }

            fn validate(self: *Branch, is_root: bool) void {
                if (is_root) {
                    assert(self.node.parent == null);
                } else {
                    const parent = self.node.getParent().?;
                    const child_ix = parent.findChild(&self.node);
                    var valid_state = config.BranchState.init();
                    valid_state.update(&self.node);
                    assert(meta.deepEqual(parent.state[child_ix], valid_state));
                }
                // TODO rebalance underfull branches
                //if (!is_root) {
                //assert(self.num_children >= @divTrunc(config.children_per_branch, 2));
                //}
                var child_ix: usize = 0;
                while (child_ix < self.num_children) : (child_ix += 1) {
                    assert(self.children[child_ix].parent == &self.node);
                    self.children[child_ix].validate();
                }
            }
        };

        pub const Point = struct {
            // Always points at a byte, unless we're at the end of the tree
            pos: usize,
            leaf: *Leaf,
            offset: Leaf.Offset,

            pub fn isAtStart(self: Point) bool {
                return self.pos == 0;
            }

            pub fn isAtEnd(self: Point) bool {
                return self.offset == self.leaf.num_items;
            }

            pub fn getNextItem(self: *Point) config.Item {
                assert(!self.isAtEnd());
                return self.leaf.items[self.offset];
            }

            const Seek = enum { Found, NotFound };

            pub fn seekNextLeaf(self: *Point) Seek {
                var node = &self.leaf.node;

                self.pos += self.leaf.num_items - self.offset;

                // go up
                while (true) {
                    if (node.getParent()) |parent| {
                        const child_ix = parent.findChild(node);
                        if (child_ix + 1 >= parent.num_children) {
                            // keep going up
                            node = &parent.node;
                        } else {
                            // go down
                            var child = parent.children[child_ix + 1];
                            while (child.tag == .Branch) {
                                const branch = child.asBranch();
                                child = branch.children[0];
                            }
                            self.leaf = child.asLeaf();
                            self.offset = 0;
                            return .Found;
                        }
                    } else {
                        self.offset = self.leaf.num_items;
                        return .NotFound;
                    }
                }
            }

            pub fn seekNextItem(self: *Point) Seek {
                if (self.offset + 1 >= self.leaf.num_items) {
                    if (self.seekNextLeaf() == .NotFound) return .NotFound;
                } else {
                    self.pos += 1;
                    self.offset += 1;
                }
                return .Found;
            }

            pub fn seekPrevLeaf(self: *Point) Seek {
                var node = &self.leaf.node;

                self.pos -= self.offset;

                // go up
                while (true) {
                    if (node.getParent()) |parent| {
                        const child_ix = parent.findChild(node);
                        if (child_ix == 0) {
                            // keep going up
                            node = &parent.node;
                        } else {
                            // go down
                            var child = parent.children[child_ix - 1];
                            while (child.tag == .Branch) {
                                const branch = child.asBranch();
                                child = branch.children[branch.num_children - 1];
                            }
                            self.leaf = child.asLeaf();
                            self.offset = @intCast(Leaf.Offset, self.leaf.num_items) - 1;
                            return .Found;
                        }
                    } else {
                        self.offset = 0;
                        return .NotFound;
                    }
                }
            }

            pub fn seekPrevItem(self: *Point) Seek {
                if (self.offset == 0) {
                    if (self.seekPrevLeaf() == .NotFound) return .NotFound;
                } else {
                    self.pos -= 1;
                    self.offset -= 1;
                }
                return .Found;
            }

            pub fn searchForwards(self: *Point, needle: []const config.Item) Seek {
                assert(needle.len > 0);
                if (self.isAtEnd()) return .NotFound;
                const needle_start_item = needle[0];
                while (true) {
                    const haystack_start_item = self.getNextItem();
                    if (haystack_start_item == needle_start_item) {
                        var end_point = self.*;
                        var is_match = true;
                        for (needle[1..]) |needle_item| {
                            if (end_point.seekNextItem() == .Found)
                                if (end_point.getNextItem() == needle_item)
                                    continue;
                            is_match = false;
                            break;
                        }
                        if (is_match) return .Found;
                    }
                    if (self.seekNextItem() == .NotFound) return .NotFound;
                }
            }

            pub fn searchBackwards(self: *Point, needle: []const config.Item) Seek {
                assert(needle.len > 0);
                const needle_start_item = needle[0];
                while (true) {
                    if (self.seekPrevItem() == .NotFound) return .NotFound;
                    const haystack_start_item = self.getNextItem();
                    if (haystack_start_item == needle_start_item) {
                        var end_point = self.*;
                        var is_match = true;
                        for (needle[1..]) |needle_item| {
                            if (end_point.seekNextItem() == .Found)
                                if (end_point.getNextItem() == needle_item)
                                    continue;
                            is_match = false;
                            break;
                        }
                        if (is_match) return .Found;
                    }
                }
            }
        };
    };
}
