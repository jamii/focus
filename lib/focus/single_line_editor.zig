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
        const buffer = Buffer.initEmpty(app);
        const editor = Editor.init(app, buffer, false, false);
        editor.insert(editor.getMainCursor(), init_text);
        editor.goBufferEnd(editor.getMainCursor());

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

        // TODO filter out return so doesn't overwrite marked text
        self.editor.frame(window, rect, events);

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
        // TODO should this copy?
        return self.buffer.bytes.items;
    }

    pub fn setText(self: *SingleLineEditor, text: []const u8) void {
        self.buffer.replace(text);
        self.editor.clearMark();
        self.editor.collapseCursors();
        self.editor.goBufferEnd(self.editor.getMainCursor());
    }
};
