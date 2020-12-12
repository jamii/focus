const focus = @import("../focus.zig");
usingnamespace focus.common;
const App = focus.App;
const Buffer = focus.Buffer;
const Editor = focus.Editor;
const Window = focus.Window;
const style = focus.style;
const Selector = focus.Selector;

pub const SingleLineEditor = struct {
    app: *App,
    buffer: *Buffer,
    editor: *Editor,

    pub fn init(app: *App, init_text: []const u8) SingleLineEditor {
        const buffer = Buffer.initEmpty(app, .Real);
        const editor = Editor.init(app, buffer, false, false);
        editor.insert(editor.getMainCursor(), init_text);
        return SingleLineEditor{
            .app = app,
            .buffer = buffer,
            .editor = editor,
        };
    }

    pub fn deinit(self: *SingleLineEditor) void {
        self.editor.deinit();
        self.buffer.deinit();
    }

    pub fn frame(self: *SingleLineEditor, window: *Window, rect: Rect, events: []const c.SDL_Event) void {
        // filter out return presses
        var editor_events = ArrayList(c.SDL_Event).init(self.app.frame_allocator);
        for (events) |event| {
            if (event.type == c.SDL_KEYDOWN and event.key.keysym.sym == c.SDLK_RETURN) continue;
            editor_events.append(event) catch oom();
        }

        // run editor
        self.editor.frame(window, rect, editor_events.items);

        // remove any sneaky newlines from eg paste
        // TODO want to put this between event handling and render
        {
            var pos: usize = 0;
            while (self.buffer.searchForwards(pos, "\n")) |new_pos| {
                pos = new_pos;
                self.editor.delete(pos, pos + 1);
            }
        }
    }

    pub fn getText(self: *SingleLineEditor) []const u8 {
        return self.buffer.tree.copy(self.app.frame_allocator, 0, self.buffer.getBufferEnd());
    }

    pub fn setText(self: *SingleLineEditor, text: []const u8) void {
        self.buffer.replace(text);
        self.editor.clearMark();
        self.editor.collapseCursors();
        self.editor.goBufferEnd(self.editor.getMainCursor());
    }
};
