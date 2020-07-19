const focus = @import("../focus.zig");
usingnamespace focus.common;
const App = focus.App;
const Id = focus.Id;
const Buffer = focus.Buffer;
const Editor = focus.Editor;
const Window = focus.Window;
const style = focus.style;
const Selector = focus.Selector;

pub const SingleLineEditor = struct {
    app: *App,
    buffer_id: Id,
    editor_id: Id,

    pub fn init(app: *App, init_text: []const u8) SingleLineEditor {
        const buffer_id = Buffer.initEmpty(app);
        const editor_id = Editor.init(app, buffer_id);
        var editor = app.getThing(editor_id).Editor;
        editor.insert(editor.getMainCursor(), init_text);
        editor.goBufferEnd(editor.getMainCursor());

        return SingleLineEditor{
            .app = app,
            .buffer_id = buffer_id,
            .editor_id = editor_id,
        };
    }

    pub fn frame(self: *SingleLineEditor, window: *Window, rect: Rect, events: []const c.SDL_Event) void {
        var buffer = self.app.getThing(self.buffer_id).Buffer;
        var editor = self.app.getThing(self.editor_id).Editor;

        // TODO filter out return so doesn't overwrite marked text

        editor.frame(window, rect, events);

        // remove any sneaky newlines from eg paste
        // TODO want to put this between event handling and render
        {
            var pos: usize = 0;
            while (buffer.searchForwards(pos, "\n")) |new_pos| {
                pos = new_pos;
                editor.delete(pos, pos + 1);
            }
        }
    }

    pub fn getText(self: *SingleLineEditor) []const u8 {
        return self.app.getThing(self.buffer_id).Buffer.bytes.items;
    }

    pub fn setText(self: *SingleLineEditor, text: []const u8) void {
        var buffer = self.app.getThing(self.buffer_id).Buffer;
        var editor = self.app.getThing(self.editor_id).Editor;
        buffer.bytes.shrink(0);
        buffer.bytes.appendSlice(text) catch oom();
        editor.clearMark();
        editor.collapseCursors();
        editor.goBufferEnd(editor.getMainCursor());
    }
};
