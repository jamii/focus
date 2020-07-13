pub const common = @import("./focus/common.zig");
pub const meta = @import("./focus/meta.zig");
pub const Atlas = @import("./focus/atlas.zig").Atlas;
pub const Buffer = @import("./focus/buffer.zig").Buffer;
pub const Editor = @import("./focus/editor.zig").Editor;
pub const FileOpener = @import("./focus/file_opener.zig").FileOpener;
pub const ProjectFileOpener = @import("./focus/project_file_opener.zig").ProjectFileOpener;
pub const BufferSearcher = @import("./focus/buffer_searcher.zig").BufferSearcher;
pub const ProjectSearcher = @import("./focus/project_searcher.zig").ProjectSearcher;
pub const Window = @import("./focus/window.zig").Window;
pub const style = @import("./focus/style.zig");

usingnamespace common;

pub fn run(allocator: *Allocator) !void {
    var app = try App.init(allocator);
    while (true) {
        try app.frame();
    }
}

pub const Tag = enum(u8) {
    Buffer,
    Editor,
    FileOpener,
    ProjectFileOpener,
    BufferSearcher,
    ProjectSearcher,
    Window,
};

pub fn tagOf(comptime thing_type: type) Tag {
    return std.meta.stringToEnum(Tag, @typeName(thing_type)).?;
}

pub const Id = struct {
    tag: Tag,
    id: u64,
};

pub const Thing = union(Tag) {
    Buffer: *Buffer,
    Editor: *Editor,
    FileOpener: *FileOpener,
    ProjectFileOpener: *ProjectFileOpener,
    BufferSearcher: *BufferSearcher,
    ProjectSearcher: *ProjectSearcher,
    Window: *Window,

    pub fn deinit(self: *Thing) void {
        inline for (@typeInfo(self).Union.fields) |field| {
            if (@enumToInt(std.meta.tag(Thing)) == field.enum_field.?.value) {
                @field(self, field.name).deinit();
            }
        }
    }
};

pub const App = struct {
    allocator: *Allocator,
    // TODO add per-frame arena allocator?
    atlas: *Atlas,
    next_id: u64,
    things: DeepHashMap(Id, Thing),
    ids: AutoHashMap(Thing, Id),

    pub fn init(allocator: *Allocator) !*App {
        if (c.SDL_Init(c.SDL_INIT_EVERYTHING) != 0)
            panic("SDL init failed: {s}", .{c.SDL_GetError()});

        var atlas = try allocator.create(Atlas);
        atlas.* = try Atlas.init(allocator);

        var self = try allocator.create(App);
        self.* = App{
            .allocator = allocator,
            .atlas = atlas,
            .next_id = 0,
            .things = DeepHashMap(Id, Thing).init(allocator),
            .ids = AutoHashMap(Thing, Id).init(allocator),
        };

        const buffer_id = try Buffer.initEmpty(self);
        const editor_id = try Editor.init(self, buffer_id);
        const window_id = try Window.init(self, editor_id);

        try self.getThing(buffer_id).Buffer.insert(0, "some initial text\nand some more\nshort\nreaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaally long" ++ ("abc\n" ** 20000));

        return self;
    }

    pub fn deinit(self: *App) void {
        var thing_iter = self.things.iterator();
        while (thing_iter.next()) |kv| {
            kv.value.deinit();
            self.allocator.destroy(kv.value);
        }
        self.atlas.deinit();
        self.allocator.destroy(self.atlas);

        self.allocator.destroy(self);
    }

    // TODO thing gc

    pub fn putThing(self: *App, thing_inner: var) !Id {
        const id = Id{
            .tag = comptime tagOf(@TypeOf(thing_inner)),
            .id = self.next_id,
        };
        self.next_id += 1;
        const thing_ptr = try self.allocator.create(@TypeOf(thing_inner));
        thing_ptr.* = thing_inner;
        const thing = @unionInit(Thing, @typeName(@TypeOf(thing_inner)), thing_ptr);
        _ = try self.things.put(id, thing);
        _ = try self.ids.put(thing, id);
        return id;
    }

    pub fn getThing(self: *App, id: Id) Thing {
        if (self.things.getValue(id)) |thing| {
            assert(std.meta.activeTag(thing) == id.tag);
            return thing;
        } else {
            panic("Missing thing: {}", .{id});
        }
    }

    pub fn getId(self: *App, thing_ptr: var) Id {
        const thing = @unionInit(Thing, @typeName(@typeInfo(@TypeOf(thing_ptr)).Pointer.child), thing_ptr);
        if (self.ids.getValue(thing)) |id| {
            assert(std.meta.activeTag(thing) == id.tag);
            return id;
        } else {
            var ids_iter = self.ids.iterator();
            while (ids_iter.next()) |kv| {
                warn("{}\n", .{kv.key});
            }
            panic("Missing id: {}", .{thing});
        }
    }

    pub fn frame(self: *App) !void {
        // fetch events
        var events = ArrayList(c.SDL_Event).init(self.allocator);
        defer events.deinit();
        var event: c.SDL_Event = undefined;
        while (c.SDL_PollEvent(&event) != 0) {
            if (event.type == c.SDL_QUIT) {
                std.os.exit(0);
            }
            try events.append(event);
        }

        // run window frames
        // collect all windows up front, because the list might change during frame
        var windows = ArrayList(*Window).init(self.allocator);
        defer windows.deinit();
        var entity_iter = self.things.iterator();
        while (entity_iter.next()) |kv| {
            if (kv.value == .Window) try windows.append(kv.value.Window);
        }
        for (windows.items) |window| {
            try window.frame(events.items);
        }

        // TODO separate frame from vsync. if vsync takes more than, say, 1s/120 then we must have missed a frame
    }
};
