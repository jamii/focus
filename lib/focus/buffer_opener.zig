const std = @import("std");
const focus = @import("../focus.zig");
const u = focus.util;
const c = focus.util.c;
const App = focus.App;
const Buffer = focus.Buffer;
const Editor = focus.Editor;
const SingleLineEditor = focus.SingleLineEditor;
const Window = focus.Window;
const style = focus.style;
const Selector = focus.Selector;
const mach_compat = focus.mach_compat;

pub const BufferOpener = struct {
    app: *App,
    ignore_buffer: ?*Buffer,
    preview_editor: *Editor,
    input: SingleLineEditor,
    selector: Selector,

    pub fn init(app: *App, ignore_buffer: ?*Buffer) *BufferOpener {
        const empty_buffer = Buffer.initEmpty(app, .{
            .limit_load_bytes = true,
            .enable_completions = false,
            .enable_undo = false,
        });
        const preview_editor = Editor.init(app, empty_buffer, .{
            .show_status_bar = false,
            .show_completer = false,
        });
        const input = SingleLineEditor.init(app, "");
        // const input = SingleLineEditor.init(app, app.last_file_filter);
        input.editor.goRealLineStart(input.editor.getMainCursor());
        input.editor.setMark();
        input.editor.goRealLineEnd(input.editor.getMainCursor());
        const selector = Selector.init(app);
        // selector.selected = app.last_buffer_opener_selected;

        const self = app.allocator.create(BufferOpener) catch u.oom();
        self.* = BufferOpener{
            .app = app,
            .ignore_buffer = ignore_buffer,
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

    pub fn frame(self: *BufferOpener, window: *Window, rect: u.Rect, events: []const mach_compat.Event) void {
        const layout = window.layoutSearcherWithPreview(rect);

        // run input frame
        const input_changed = self.input.frame(window, layout.input, events);
        if (input_changed == .Changed) self.selector.selected = 0;

        // get buffer paths
        var paths = u.ArrayList([]const u8).init(self.app.frame_allocator);
        {
            const Entry = @TypeOf(self.app.buffers).Entry;
            var entries = u.ArrayList(Entry).init(self.app.frame_allocator);
            var buffers_iter = self.app.buffers.iterator();
            while (buffers_iter.next()) |entry| {
                if (entry.value_ptr.* != self.ignore_buffer)
                    entries.append(entry) catch u.oom();
            }
            // sort by most recently focused
            std.mem.sort(Entry, entries.items, {}, (struct {
                fn lessThan(_: void, a: Entry, b: Entry) bool {
                    return b.value_ptr.*.last_lost_focus_ms < a.value_ptr.*.last_lost_focus_ms;
                }
            }).lessThan);
            for (entries.items) |entry| {
                paths.append(entry.key_ptr.*) catch u.oom();
            }
        }

        // filter paths
        const filtered_paths = u.fuzzy_search(self.app.frame_allocator, paths.items, self.input.getText());

        // run selector frame
        self.selector.setItems(filtered_paths);
        const action = self.selector.frame(window, layout.selector, events);

        // maybe open file
        if (action == .SelectOne) {
            const path = filtered_paths[self.selector.selected];
            const new_buffer = self.app.getBufferFromAbsoluteFilename(path);
            const new_editor = Editor.init(self.app, new_buffer, .{});
            window.popView();
            window.pushView(new_editor);
        }

        // set cached search text
        self.app.allocator.free(self.app.last_file_filter);
        self.app.last_file_filter = self.app.dupe(self.input.getText());
        self.app.last_buffer_opener_selected = self.selector.selected;

        // update preview
        self.preview_editor.deinit();
        if (self.selector.selected >= filtered_paths.len) {
            const empty_buffer = Buffer.initEmpty(self.app, .{
                .limit_load_bytes = true,
                .enable_completions = false,
                .enable_undo = false,
            });
            self.preview_editor = Editor.init(self.app, empty_buffer, .{
                .show_status_bar = false,
                .show_completer = false,
            });
        } else {
            const selected = filtered_paths[self.selector.selected];
            const preview_buffer = self.app.getBufferFromAbsoluteFilename(selected);
            self.preview_editor = Editor.init(self.app, preview_buffer, .{
                .show_status_bar = false,
                .show_completer = false,
            });
        }

        // run preview frame
        self.preview_editor.frame(window, layout.preview, &[0]mach_compat.Event{});
    }
};
