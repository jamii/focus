const focus = @import("../focus.zig");
usingnamespace focus.common;
const meta = focus.meta;
const App = focus.App;
const Id = focus.Id;
const Buffer = focus.Buffer;
const Editor = focus.Editor;
const Window = focus.Window;

pub const Selector = struct {
    app: *App,
    buffer_id: Id,
    editor_id: Id,
    selected: usize,

    pub const Action = enum {
        None,
        SelectRaw,
        SelectOne,
        SelectAll,
    };

    pub fn init(app: *App) Selector {
        const buffer_id = Buffer.initEmpty(app);
        const editor_id = Editor.init(app, buffer_id);
        return Selector{
            .app = app,
            .buffer_id = buffer_id,
            .editor_id = editor_id,
            .selected = 0,
        };
    }

    pub fn deinit(self: *Selector) void {}

    pub fn frame(self: *Selector, window: *Window, rect: Rect, events: []const c.SDL_Event, items: []const []const u8) Action {
        var buffer = self.app.getThing(self.buffer_id).Buffer;
        var editor = self.app.getThing(self.editor_id).Editor;

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
        buffer.bytes.shrink(0);
        for (items) |item| {
            buffer.bytes.appendSlice(item) catch oom();
            buffer.bytes.append('\n') catch oom();
        }

        // set selection
        self.selected = min(self.selected, max(1, items.len) - 1);
        var cursor = editor.getMainCursor();
        if (items.len != 0) {
            editor.goPos(cursor, buffer.getPosForLineCol(self.selected, 0));
            editor.setMark();
            editor.goLineEnd(cursor);
        } else {
            editor.clearMark();
            editor.goBufferStart(cursor);
        }

        // render
        editor.frame(window, rect, &[0]c.SDL_Event{});

        return action;
    }
};
