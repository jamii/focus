const focus = @import("../focus.zig");
usingnamespace focus.common;
const meta = focus.meta;
const App = focus.App;
const Buffer = focus.Buffer;
const Editor = focus.Editor;
const Window = focus.Window;

pub const Selector = struct {
    app: *App,
    buffer: *Buffer,
    editor: *Editor,
    selected: usize,

    pub const Action = enum {
        None,
        SelectRaw,
        SelectOne,
        SelectAll,
    };

    pub fn init(app: *App) Selector {
        const buffer = Buffer.initEmpty(app, .Real);
        const editor = Editor.init(app, buffer, false, false);
        return Selector{
            .app = app,
            .buffer = buffer,
            .editor = editor,
            .selected = 0,
        };
    }

    pub fn deinit(self: *Selector) void {
        self.editor.deinit();
        self.buffer.deinit();
    }

    pub fn frame(self: *Selector, window: *Window, rect: Rect, events: []const c.SDL_Event, items: []const []const u8) Action {
        var action: Action = .None;

        // handle events
        for (events) |event| {
            switch (event.type) {
                c.SDL_KEYDOWN => {
                    const sym = event.key.keysym;
                    if (sym.mod == c.KMOD_LCTRL or sym.mod == c.KMOD_RCTRL) {
                        switch (sym.sym) {
                            'k' => self.selected += 1,
                            'i' => if (self.selected != 0) {
                                self.selected -= 1;
                            },
                            c.SDLK_RETURN => {
                                action = .SelectRaw;
                            },
                            else => {},
                        }
                    } else if (sym.mod == c.KMOD_LALT or sym.mod == c.KMOD_RALT) {
                        switch (sym.sym) {
                            'k' => self.selected = items.len - 1,
                            'i' => self.selected = 0,
                            c.SDLK_RETURN => {
                                action = .SelectAll;
                            },
                            else => {},
                        }
                    } else if (sym.mod == 0) {
                        switch (sym.sym) {
                            c.SDLK_RETURN => if (items.len != 0) {
                                action = .SelectOne;
                            },
                            else => {},
                        }
                    }
                },
                else => {},
            }
        }

        // fill buffer
        // TODO highlight found area
        var text = ArrayList(u8).init(self.app.frame_allocator);
        for (items) |item| {
            text.appendSlice(item) catch oom();
            text.append('\n') catch oom();
        }
        self.buffer.replace(text.items);

        // set selection
        self.selected = min(self.selected, max(1, items.len) - 1);
        var cursor = self.editor.getMainCursor();
        if (items.len != 0) {
            self.editor.goPos(cursor, self.buffer.getPosForLineCol(self.selected, 0));
            self.editor.setMark();
            self.editor.goRealLineEnd(cursor);
        } else {
            self.editor.clearMark();
            self.editor.goBufferStart(cursor);
        }

        // render
        self.editor.frame(window, rect, &[0]c.SDL_Event{});

        return action;
    }
};
