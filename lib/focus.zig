pub const common = @import("./focus/common.zig");
pub const meta = @import("./focus/meta.zig");
pub const Atlas = @import("./focus/atlas.zig").Atlas;
pub const Buffer = @import("./focus/buffer.zig").Buffer;
pub const View = @import("./focus/view.zig").View;
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
    buffer: *Buffer,
    view: View,

    pub fn init(allocator: *Allocator) ! App {
        if (c.SDL_Init(c.SDL_INIT_EVERYTHING) != 0)
            panic("SDL init failed: {s}", .{c.SDL_GetError()});
        
        var atlas = try allocator.create(Atlas);
        atlas.* = try Atlas.init(allocator);
        var window = try allocator.create(Window);
        window.* = Window.init(allocator, atlas);
        var buffer = try allocator.create(Buffer);
        buffer.* = Buffer.init(allocator);
        try buffer.insert(0, "some initial text\nand some more\nshort\nreaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaally long" ++ ("abc\n"**20000));
        var view = try View.init(allocator, buffer);
        
        return App{
            .allocator = allocator,
            .atlas = atlas,
            .window = window,
            .buffer = buffer,
            .view = view,
        };
    }

    pub fn deinit(self: *App) void {
        self.view.deinit();
        self.allocator.destroy(self.view);
        self.buffer.deinit();
        self.allocator.destroy(self.buffer);
        self.window.deinit();
        self.allocator.destroy(self.window);
        self.atlas.deinit();
    }

    pub fn frame(self: *App) ! void {
        // var timer = try std.time.Timer.start();

        // fetch events
        var event: c.SDL_Event = undefined;
        while (c.SDL_PollEvent(&event) != 0) {
            if (event.type == c.SDL_QUIT) {
                std.os.exit(0);
            }
            // TODO only one window for now
            try self.window.events.append(event);
        }

        // run editor frame
        const screen_rect = try self.window.begin();
        try self.view.frame(self.window, screen_rect);
        try self.window.end();
        
        // warn("frame time: {}ns\n", .{timer.read()});
    }
};
