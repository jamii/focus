pub const common = @import("./focus/common.zig");
pub const meta = @import("./focus/meta.zig");
pub const Atlas = @import("./focus/atlas.zig").Atlas;
pub const Buffer = @import("./focus/buffer.zig").Buffer;
pub const Editor = @import("./focus/editor.zig").Editor;
pub const FileOpener = @import("./focus/file_opener.zig").FileOpener;
pub const Window = @import("./focus/window.zig").Window;

usingnamespace common;

pub fn run(allocator: *Allocator) !void {
    var app = try App.init(allocator);
    while (true) {
        try app.frame();
    }
}

pub const Tag = packed enum(u8) {
    Buffer,
    Editor,
    FileOpener,
    Window,

    comptime {
        assert(@sizeOf(@This()) == 1);
    }
};

pub fn tagOf(comptime thing_type: type) Tag {
    return std.meta.stringToEnum(Tag, @typeName(thing_type)).?;
}

pub const Id = packed struct {
    id: u56,
    tag: Tag,

    comptime {
        assert(@sizeOf(@This()) == 8);
    }
};

pub const Thing = union(Tag) {
    Buffer: *Buffer,
    Editor: *Editor,
    FileOpener: *FileOpener,
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
    atlas: *Atlas,
    next_id: u56,
    things: DeepHashMap(Id, Thing),

    pub fn init(allocator: *Allocator) ! *App {
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
        };

        const buffer_id = try Buffer.init(self);
        const editor_id = try Editor.init(self, buffer_id);
        const window_id = try Window.init(self, editor_id);

        try self.getThing(buffer_id).Buffer.insert(0, "some initial text\nand some more\nshort\nreaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaally long" ++ ("abc\n"**20000));
        
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

    // TODO entity gc
    
    pub fn putThing(self: *App, thing: var) ! Id {
        const id = Id{
            .tag = comptime tagOf(@TypeOf(thing)),
            .id = self.next_id,
        };
        self.next_id += 1;
        const thing_ptr = try self.allocator.create(@TypeOf(thing));
        thing_ptr.* = thing;
        _ = try self.things.put(id, @unionInit(Thing, @typeName(@TypeOf(thing)), thing_ptr));
        return id;
    }

    pub fn getThing(self: *App, id: Id) Thing {
        _ = self.things.getValue(id); // TODO this is a workaround for some kind of codegen bug where the first call to getValue is sometimes null
        if (self.things.getValue(id)) |thing| {
            assert(std.meta.activeTag(thing) == id.tag);
            return thing;
        } else {
            panic("Missing thing: {}", .{id});
        }
    }

    pub fn frame(self: *App) ! void {
        // var timer = try std.time.Timer.start();

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
        
        // warn("frame time: {}ns\n", .{timer.read()});
    }
};
