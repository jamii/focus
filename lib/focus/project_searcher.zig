const focus = @import("../focus.zig");
usingnamespace focus.common;
const meta = focus.meta;
const App = focus.App;
const Id = focus.Id;
const Buffer = focus.Buffer;
const Editor = focus.Editor;
const Window = focus.Window;
const style = focus.style;
const Selector = focus.Selector;

pub const ProjectSearcher = struct {
    app: *App,
    project_dir: []const u8,
    preview_buffer_id: Id,
    preview_editor_id: Id,
    input_buffer_id: Id,
    input_editor_id: Id,
    selector: Selector,

    pub fn init(app: *App, project_dir: []const u8, init_filter: []const u8) Id {
        // TODO don't directly mutate buffer - messes up multiple cursors - go via editor instead
        const preview_buffer_id = Buffer.initEmpty(app);
        const preview_editor_id = Editor.init(app, preview_buffer_id);

        const input_buffer_id = Buffer.initEmpty(app);
        const input_editor_id = Editor.init(app, input_buffer_id);
        var input_editor = app.getThing(input_editor_id).Editor;
        input_editor.insert(input_editor.getMainCursor(), init_filter);
        input_editor.goBufferEnd(input_editor.getMainCursor());

        const selector = Selector.init(app);

        return app.putThing(ProjectSearcher{
            .app = app,
            .project_dir = project_dir,
            .preview_buffer_id = preview_buffer_id,
            .preview_editor_id = preview_editor_id,
            .input_buffer_id = input_buffer_id,
            .input_editor_id = input_editor_id,
            .selector = selector,
        });
    }

    pub fn deinit(self: *ProjectSearcher) void {
    }

    pub fn frame(self: *ProjectSearcher, window: *Window, rect: Rect, events: []const c.SDL_Event) void {
        var preview_buffer = self.app.getThing(self.preview_buffer_id).Buffer;
        var preview_editor = self.app.getThing(self.preview_editor_id).Editor;
        var input_buffer = self.app.getThing(self.input_buffer_id).Buffer;
        var input_editor = self.app.getThing(self.input_editor_id).Editor;

        // handle events
        var input_events = ArrayList(c.SDL_Event).init(self.app.frame_allocator);
        var selector_events = ArrayList(c.SDL_Event).init(self.app.frame_allocator);
        for (events) |event| {
            switch (event.type) {
                c.SDL_KEYDOWN => {
                    const sym = event.key.keysym;
                    if (sym.mod == c.KMOD_LCTRL or sym.mod == c.KMOD_RCTRL) {
                        switch (sym.sym) {
                            'q' => window.popView(),
                            'k', 'i', c.SDLK_RETURN => selector_events.append(event) catch oom(),
                            else => input_events.append(event) catch oom(),
                        }
                    } else if (sym.mod == c.KMOD_LALT or sym.mod == c.KMOD_RALT) {
                        switch (sym.sym) {
                            'k', 'i', c.SDLK_RETURN => selector_events.append(event) catch oom(),
                            else => input_events.append(event) catch oom(),
                        }
                    } else if (sym.mod == 0) {
                        switch (sym.sym) {
                            // TODO c.SDLK_TAB => complete prefix
                            c.SDLK_RETURN => selector_events.append(event) catch oom(),
                            else => input_events.append(event) catch oom(),
                        }
                    } else {
                        input_events.append(event) catch oom();
                    }
                },
                c.SDL_MOUSEWHEEL => {},
                else => input_events.append(event) catch oom(),
            }
        }

        // split rect
        var all_rect = rect;
        const preview_rect = all_rect.splitTop(@divTrunc(rect.h, 2), 0);
        const border1_rect = all_rect.splitTop(@divTrunc(self.app.atlas.char_height, 8), 0);
        const input_rect = all_rect.splitBottom(self.app.atlas.char_height, 0);
        const border2_rect = all_rect.splitBottom(@divTrunc(self.app.atlas.char_height, 8), 0);
        const selector_rect = all_rect;
        window.queueRect(border1_rect, style.text_color);
        window.queueRect(border2_rect, style.text_color);

        // run input frame
        input_editor.frame(window, input_rect, input_events.toOwnedSlice());

        // remove any sneaky newlines
        {
            var pos: usize = 0;
            while (input_buffer.searchForwards(pos, "\n")) |new_pos| {
                pos = new_pos;
                input_editor.delete(pos, pos + 1);
            }
        }

        // get and filter results
        var results = ArrayList([]const u8).init(self.app.frame_allocator);
        {
            const max_line_string = format(self.app.frame_allocator, "{}", .{preview_buffer.countLines()});
            const filter = input_buffer.bytes.items;
            if (filter.len > 0) {
                const result = std.ChildProcess.exec(.{
                    .allocator = self.app.frame_allocator,
                    // TODO would prefer null separated but tricky to parse
                    .argv = &[6][]const u8{"rg", "--line-number", "--sort", "path", "--fixed-strings", filter},
                    .cwd = self.project_dir,
                    .max_output_bytes = 128 * 1024 * 1024,
                }) catch |err| panic("{} while calling rg", .{err});
                assert(result.term == .Exited); // exits with 1 if no search results
                var lines = std.mem.split(result.stdout, "\n");
                while (lines.next()) |line| {
                    if (line.len != 0) results.append(line) catch oom();
                }
            }
        }

        // run selector frame
        const action = self.selector.frame(window, selector_rect, selector_events.toOwnedSlice(), results.items);

        // update preview
        preview_editor.collapseCursors();
        preview_editor.clearMark();
        if (results.items.len > 0) {
            const line = results.items[self.selector.selected];
            var parts = std.mem.split(line, ":");
            const path_suffix = parts.next().?;
            const line_number_string = parts.next().?;
            const line_number = std.fmt.parseInt(usize, line_number_string, 10)
                catch |err| panic("{} while parsing line number {s} from rg", .{err, line_number_string});

            const path = std.fs.path.join(self.app.frame_allocator, &[2][]const u8{self.project_dir, path_suffix}) catch oom();
            preview_buffer.load(path);

            var cursor = preview_editor.getMainCursor();
            preview_editor.goLine(cursor, line_number - 1);
            preview_editor.setMark();
            preview_editor.goLineEnd(cursor);
            // TODO centre cursor

            if (action == .SelectOne) {
                window.popView();
                window.pushView(self.preview_editor_id);
            }
        }

        // run preview frames
        preview_editor.frame(window, preview_rect, &[0]c.SDL_Event{});
    }
};
