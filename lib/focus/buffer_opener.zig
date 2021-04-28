const focus = @import("../focus.zig");
usingnamespace focus.common;
const meta = focus.meta;
const App = focus.App;
const Buffer = focus.Buffer;
const Editor = focus.Editor;
const SingleLineEditor = focus.SingleLineEditor;
const Window = focus.Window;
const style = focus.style;
const Selector = focus.Selector;

pub const BufferOpener = struct {
    app: *App,
    preview_editor: *Editor,
    input: SingleLineEditor,
    selector: Selector,

    pub fn init(app: *App) *BufferOpener {
        const empty_buffer = Buffer.initEmpty(app, .Preview);
        const preview_editor = Editor.init(app, empty_buffer, false, false);
        const input = SingleLineEditor.init(app, "");
        const selector = Selector.init(app);

        const self = app.allocator.create(BufferOpener) catch oom();
        self.* = BufferOpener{
            .app = app,
            .preview_editor = preview_editor,
            .input = input,
            .selector = selector,
        };
        return self;
    }

    pub fn deinit(self: *BufferOpener) void {
        self.selector.deinit();
        self.input.deinit();
        self.preview_editor.deinit();
        self.app.allocator.destroy(self);
    }

    pub fn frame(self: *BufferOpener, window: *Window, rect: Rect, events: []const c.SDL_Event) void {
        const layout = window.layoutSearcherWithPreview(rect);

        // run input frame
        self.input.frame(window, layout.input, events);

        // filter paths
        var paths = ArrayList([]const u8).init(self.app.frame_allocator);
        {
            var buffers_iter = self.app.buffers.iterator();
            while (buffers_iter.next()) |kv| {
                paths.append(kv.key) catch oom();
            }
        }
        const filtered_paths = fuzzy_search(self.app.frame_allocator, paths.items, self.input.getText());

        // run selector frame
        const action = self.selector.frame(window, layout.selector, events, filtered_paths);

        // maybe open file
        if (action == .SelectOne and filtered_paths.len > 0) {
            const path = filtered_paths[self.selector.selected];
            const new_buffer = self.app.getBufferFromAbsoluteFilename(path);
            const new_editor = Editor.init(self.app, new_buffer, true, true);
            window.popView();
            window.pushView(new_editor);
        }

        // update preview
        self.preview_editor.deinit();
        if (filtered_paths.len == 0) {
            const empty_buffer = Buffer.initEmpty(self.app, .Preview);
            self.preview_editor = Editor.init(self.app, empty_buffer, false, false);
        } else {
            const selected = filtered_paths[self.selector.selected];
            const preview_buffer = self.app.getBufferFromAbsoluteFilename(selected);
            self.preview_editor = Editor.init(self.app, preview_buffer, false, false);
        }

        // run preview frame
        self.preview_editor.frame(window, layout.preview, &[0]c.SDL_Event{});
    }
};
