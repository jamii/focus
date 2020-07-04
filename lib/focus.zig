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

pub const App = struct {
    allocator: *Allocator,
    atlas: *Atlas,
    window: *Window,

    pub fn init(allocator: *Allocator) ! App {
        if (c.SDL_Init(c.SDL_INIT_EVERYTHING) != 0)
            panic("SDL init failed: {s}", .{c.SDL_GetError()});
        
        var atlas = try allocator.create(Atlas);
        atlas.* = try Atlas.init(allocator);
        var buffer = try allocator.create(Buffer);
        buffer.* = Buffer.init(allocator);
        try buffer.insert(0, "some initial text\nand some more\nshort\nreaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaally long" ++ ("abc\n"**20000));
        var editor = try Editor.init(allocator, buffer);
        var window = try allocator.create(Window);

        var self = App{
            .allocator = allocator,
            .atlas = atlas,
            .window = window,
        };
        
        self.window.* = try Window.init(allocator, &self, .{.Editor = editor});
        
        return self;
    }

    pub fn deinit(self: *App) void {
        self.editor.deinit();
        self.allocator.destroy(self.editor);
        self.buffer.deinit();
        self.allocator.destroy(self.buffer);
        self.window.deinit();
        self.allocator.destroy(self.window);
        self.atlas.deinit();
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

        // run window frame
        try self.window.frame(self, events.items);
        
        // warn("frame time: {}ns\n", .{timer.read()});
    }
};
