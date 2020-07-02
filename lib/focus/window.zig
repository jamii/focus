const focus = @import("../focus.zig");
usingnamespace focus.common;
const Atlas = focus.Atlas;

pub const Window = struct {
    allocator: *Allocator,
    atlas: *Atlas, // borrowed
    
    sdl_window: *c.SDL_Window,
    width: Coord,
    height: Coord,
    events: ArrayList(c.SDL_Event),
    commands: ArrayList(Command),
    
    gl_context: c.SDL_GLContext,
    texture_buffer: ArrayList(Quad(Vec2f)),
    vertex_buffer: ArrayList(Quad(Vec2f)),
    color_buffer: ArrayList(Quad(Color)),
    index_buffer: ArrayList([2]Tri(u32)),

    pub const Command = union(enum) {
        Rect: struct {
            rect: Rect,
            color: Color,
        },
        Text: struct {
            pos: Vec2,
            color: Color,
            chars: []const u8,
        },
    };

    pub fn init(allocator: *Allocator, atlas: *Atlas) Window {
        // pretty arbitrary
        const init_width: usize = 1920;
        const init_height: usize = 1080;

        // init window
        const sdl_window = c.SDL_CreateWindow(
            "focus",
            c.SDL_WINDOWPOS_UNDEFINED,
            c.SDL_WINDOWPOS_UNDEFINED,
            @as(c_int, init_width),
            @as(c_int, init_height),
            c.SDL_WINDOW_OPENGL | c.SDL_WINDOW_BORDERLESS | c.SDL_WINDOW_ALLOW_HIGHDPI | c.SDL_WINDOW_RESIZABLE,
        ) orelse panic("SDL window creation failed: {s}", .{c.SDL_GetError()});
        // on pinephone, fullscreen
        // if (std.Target.current.cpu.arch == .aarch64) {
        //     _ = SDL_SetWindowFullscreen(window, SDL_WINDOW_FULLSCREEN);
        // }

        // init gl
        const gl_context = c.SDL_GL_CreateContext(sdl_window);
        if (c.SDL_GL_MakeCurrent(sdl_window, gl_context) != 0)
            panic("Switching to GL context failed: {s}",  .{c.SDL_GetError()});
        c.glEnable(c.GL_BLEND);
        c.glBlendFunc(c.GL_SRC_ALPHA, c.GL_ONE_MINUS_SRC_ALPHA);
        c.glDisable(c.GL_CULL_FACE);
        c.glDisable(c.GL_DEPTH_TEST);
        c.glEnable(c.GL_TEXTURE_2D);
        c.glEnableClientState(c.GL_VERTEX_ARRAY);
        c.glEnableClientState(c.GL_TEXTURE_COORD_ARRAY);
        c.glEnableClientState(c.GL_COLOR_ARRAY);

        // init texture
        var id: u32 = undefined;
        c.glGenTextures(1, &id);
        c.glBindTexture(c.GL_TEXTURE_2D, id);
        c.glTexImage2D(c.GL_TEXTURE_2D, 0, c.GL_ALPHA, atlas.texture_dims.x, atlas.texture_dims.y, 0, c.GL_RGBA, c.GL_UNSIGNED_BYTE, atlas.texture.ptr);
        c.glTexParameteri(c.GL_TEXTURE_2D, c.GL_TEXTURE_MIN_FILTER, c.GL_NEAREST);
        c.glTexParameteri(c.GL_TEXTURE_2D, c.GL_TEXTURE_MAG_FILTER, c.GL_NEAREST);
        assert(c.glGetError() == 0);

        // sync with monitor - causes input lag
        if (c.SDL_GL_SetSwapInterval(1) != 0)
            panic("Setting swap interval failed: {}",  .{c.SDL_GetError()});

        // accept unicode input
        c.SDL_StartTextInput();
        
        return Window{
            .allocator = allocator,
            .atlas = atlas,
            
            .sdl_window = sdl_window,
            .width = init_width,
            .height = init_height,
            .events = ArrayList(c.SDL_Event).init(allocator),
            .commands = ArrayList(Command).init(allocator),
            
            .gl_context = gl_context,
            .texture_buffer = ArrayList(Quad(Vec2f)).init(allocator),
            .vertex_buffer = ArrayList(Quad(Vec2f)).init(allocator),
            .color_buffer = ArrayList(Quad(Color)).init(allocator),
            .index_buffer = ArrayList([2]Tri(u32)).init(allocator),
        };
    }

    pub fn deinit(self: *Window) void {
        self.index_buffer.deinit();
        self.color_buffer.deinit();
        self.vertex_buffer.deinit();
        self.texture_buffer.deinit();
        c.SDL_GL_DeleteContext(self.gl_context);
        
        self.commands.deinit();
        self.events.deinit();
        c.SDL_DestroyWindow(self.window);
        
        // atlas doesn't belong to us
    }

    pub fn begin(self: *Window) ! Rect {
        var w: c_int = undefined;
        var h: c_int = undefined;
        c.SDL_GL_GetDrawableSize(self.sdl_window, &w, &h);
        self.width = @intCast(Coord, w);
        self.height = @intCast(Coord, h);
        return Rect{ .x = 0, .y = 0, .w = self.width, .h = self.height };
    }

    pub fn end(self: *Window) ! void {
        // convert draw commands into quads
        for (self.commands.items) |command| {
            switch (command) {
                .Rect => |rect| {
                    try self.renderQuad(rect.rect, self.atlas.white_rect, rect.color);
                },
                .Text => |text| {
                    // TODO going to need to be able to clip text
                    var dst: Rect = .{ .x = text.pos.x, .y = text.pos.y, .w = 0, .h = 0 };
                    for (text.chars) |char| {
                        const src = self.atlas.char_to_rect[char];
                        dst.w = src.w;
                        dst.h = src.h;
                        try self.renderQuad(dst, src, text.color);
                        dst.x += src.w;
                    }
                }
            }
        }

        // actual rendering
        
        if (c.SDL_GL_MakeCurrent(self.sdl_window, self.gl_context) != 0)
            panic("Switching to GL context failed: {s}",  .{c.SDL_GetError()});
        
        c.glClearColor(0, 0, 0, 1);
        c.glClear(c.GL_COLOR_BUFFER_BIT);

        c.glViewport(0, 0, self.width, self.height);
        c.glMatrixMode(c.GL_PROJECTION);
        c.glPushMatrix();
        c.glLoadIdentity();
        c.glOrtho(0.0, @intToFloat(f32, self.width), @intToFloat(f32, self.height), 0.0, -1.0, 1.0);
        c.glMatrixMode(c.GL_MODELVIEW);
        c.glPushMatrix();
        c.glLoadIdentity();

        c.glTexCoordPointer(2, c.GL_FLOAT, 0, self.texture_buffer.items.ptr);
        c.glVertexPointer(2, c.GL_FLOAT, 0, self.vertex_buffer.items.ptr);
        c.glColorPointer(4, c.GL_UNSIGNED_BYTE, 0, self.color_buffer.items.ptr);
        c.glDrawElements(c.GL_TRIANGLES, @intCast(c_int, self.index_buffer.items.len) * 6, c.GL_UNSIGNED_INT, self.index_buffer.items.ptr);

        c.glMatrixMode(c.GL_MODELVIEW);
        c.glPopMatrix();
        c.glMatrixMode(c.GL_PROJECTION);
        c.glPopMatrix();

        // TODO is this going to be a problem with multiple windows?
        // looks like it - https://stackoverflow.com/questions/29617370/multiple-opengl-contexts-multiple-windows-multithreading-and-vsync
        c.SDL_GL_SwapWindow(self.sdl_window);

        // reset
        try self.events.resize(0);
        try self.commands.resize(0);
        try self.texture_buffer.resize(0);
        try self.vertex_buffer.resize(0);
        try self.color_buffer.resize(0);
        try self.index_buffer.resize(0);
    }

    fn renderQuad(self: *Window, dst: Rect, src: Rect, color: Color) ! void {
        const tx = @intToFloat(f32, src.x) / @intToFloat(f32, self.atlas.texture_dims.x);
        const ty = @intToFloat(f32, src.y) / @intToFloat(f32, self.atlas.texture_dims.y);
        const tw = @intToFloat(f32, src.w) / @intToFloat(f32, self.atlas.texture_dims.x);
        const th = @intToFloat(f32, src.h) / @intToFloat(f32, self.atlas.texture_dims.y);
        try self.texture_buffer.append(.{
            .tl = .{ .x = tx, .y = ty },
            .tr = .{ .x = tx + tw, .y = ty },
            .bl = .{ .x = tx, .y = ty + th },
            .br = .{ .x = tx + tw, .y = ty + th },
        });

        const vx = @intToFloat(f32, dst.x);
        const vy = @intToFloat(f32, dst.y);
        const vw = @intToFloat(f32, dst.w);
        const vh = @intToFloat(f32, dst.h);
        try self.vertex_buffer.append(.{
            .tl = .{ .x = vx, .y = vy },
            .tr = .{ .x = vx + vw, .y = vy },
            .bl = .{ .x = vx, .y = vy + vh },
            .br = .{ .x = vx + vw, .y = vy + vh },
        });

        try self.color_buffer.append(.{
            .tl = color,
            .tr = color,
            .bl = color,
            .br = color,
        });

        const vertex_ix = @intCast(u32, self.index_buffer.items.len * 4);
        try self.index_buffer.append(.{
            .{
                .a = vertex_ix + 0,
                .b = vertex_ix + 1,
                .c = vertex_ix + 2,
            },
            .{
                .a = vertex_ix + 2,
                .b = vertex_ix + 3,
                .c = vertex_ix + 1,
            },
        });
    }

    // drawing api

    pub fn queueRect(self: *Window, rect: Rect, color: Color) !void {
        try self.commands.append(.{
            .Rect = .{
                .rect = rect,
                .color = color,
            },
        });
    }

    pub fn queueText(self: *Window, pos: Vec2, color: Color, chars: []const u8) !void {
        try self.commands.append(.{
            .Text = .{
                .pos = pos,
                .color = color,
                .chars = chars,
            },
        });
    }

    // pub fn text(self: *Window, rect: Rect, color: Color, chars: []const u8) !void {
    //     var h: Coord = 0;
    //     var line_begin: usize = 0;
    //     while (true) {
    //         var line_end = line_begin;
    //         {
    //             var w: Coord = 0;
    //             var i: usize = line_end;
    //             while (true) {
    //                 if (i >= chars.len) {
    //                     line_end = i;
    //                     break;
    //                 }
    //                 const char = chars[i];
    //                 w += @intCast(Coord, self.atlas.max_char_width);
    //                 if (w > rect.w) {
    //                     // if haven't soft wrapped yet, hard wrap before this char
    //                     if (line_end == line_begin) {
    //                         line_end = i;
    //                     }
    //                     break;
    //                 }
    //                 if (char == '\n') {
    //                     // commit to drawing this char and wrap here
    //                     line_end = i + 1;
    //                     break;
    //                 }
    //                 if (char == ' ') {
    //                     // commit to drawing this char
    //                     line_end = i + 1;
    //                 }
    //                 // otherwise keep looking ahead
    //                 i += 1;
    //             }
    //         }
    //         try self.queueText(.{ .x = rect.x, .y = rect.y + h }, color, chars[line_begin..line_end]);
    //         line_begin = line_end;
    //         h += atlas.text_height;
    //         if (line_begin >= chars.len or h > rect.h) {
    //             break;
    //         }
    //     }
    // }
};
