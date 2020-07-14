const focus = @import("../focus.zig");
usingnamespace focus.common;
const meta = focus.meta;
const App = focus.App;
const Id = focus.Id;
const Buffer = focus.Buffer;
const Editor = focus.Editor;
const Window = focus.Window;
const style = focus.style;

// TODO arena allocator for lifespan of opener

// TODO always have a selection, open first match on enter (reject if no match), open raw text on ctrl-enter

pub const ProjectSearcher = struct {
    app: *App,
    project_dir: []const u8,
    target_buffer_id: Id,
    target_editor_id: Id,
    input_buffer_id: Id,
    input_editor_id: Id,
    completions_buffer_id: Id,
    completions_editor_id: Id,
    selected: usize, // 0 for nothing selected, i-1 for line i

    pub fn init(app: *App, project_dir: []const u8, init_filter: []const u8) Id {
        // TODO don't directly mutate buffer - messes up multiple cursors - go via editor instead
        const target_buffer_id = Buffer.initEmpty(app);
        const target_editor_id = Editor.init(app, target_buffer_id);
        const input_buffer_id = Buffer.initEmpty(app);
        const input_editor_id = Editor.init(app, input_buffer_id);
        const completions_buffer_id = Buffer.initEmpty(app);
        const completions_editor_id = Editor.init(app, completions_buffer_id);

        // set initial filter
        app.getThing(input_buffer_id).Buffer.insert(0, init_filter);

        // start cursor at end
        var input_editor = app.getThing(input_editor_id).Editor;
        input_editor.goBufferEnd(input_editor.getMainCursor());

        return app.putThing(ProjectSearcher{
            .app = app,
            .project_dir = project_dir,
            .target_buffer_id = target_buffer_id,
            .target_editor_id = target_editor_id,
            .input_buffer_id = input_buffer_id,
            .input_editor_id = input_editor_id,
            .completions_buffer_id = completions_buffer_id,
            .completions_editor_id = completions_editor_id,
            .selected = 0,
        });
    }

    pub fn deinit(self: *ProjectSearcher) void {
    }

    pub fn frame(self: *ProjectSearcher, window: *Window, rect: Rect, events: []const c.SDL_Event) void {
        var target_buffer = self.app.getThing(self.target_buffer_id).Buffer;
        var target_editor = self.app.getThing(self.target_editor_id).Editor;
        var input_buffer = self.app.getThing(self.input_buffer_id).Buffer;
        var input_editor = self.app.getThing(self.input_editor_id).Editor;
        var completions_buffer = self.app.getThing(self.completions_buffer_id).Buffer;
        var completions_editor = self.app.getThing(self.completions_editor_id).Editor;

        const Action = enum {
            None,
            SelectOne,
            // SelectAll, // TODO needs some kind of multi-editor
        };
        var action: Action = .None;

        // handle events
        var input_editor_events = ArrayList(c.SDL_Event).init(self.app.frame_allocator);
        for (events) |event| {
            var delegate = false;
            switch (event.type) {
                c.SDL_KEYDOWN => {
                    const sym = event.key.keysym;
                    if (sym.mod == c.KMOD_LCTRL or sym.mod == c.KMOD_RCTRL) {
                        switch (sym.sym) {
                            'q' => window.popView(),
                            'k' => self.selected += 1,
                            'i' => if (self.selected != 0) {
                                self.selected -= 1;
                            },
                            else => delegate = true,
                        }
                    } else if (sym.mod == c.KMOD_LALT or sym.mod == c.KMOD_RALT) {
                        switch (sym.sym) {
                            'k' => self.selected = completions_buffer.countLines() - 1,
                            'i' => self.selected = 1,
                            // c.SDLK_RETURN => {
                            //     action = .SelectAll;
                            // },
                            else => delegate = true,
                        }
                    } else if (sym.mod == 0) {
                        switch (sym.sym) {
                            c.SDLK_RETURN => {
                                action = .SelectOne;
                            },
                            else => delegate = true,
                        }
                    } else {
                        delegate = true;
                    }
                },
                c.SDL_MOUSEWHEEL => {},
                else => delegate = true,
            }
            // delegate other events to input editor
            if (delegate) input_editor_events.append(event) catch oom();
        }

        // remove any sneaky newlines
        {
            var pos: usize = 0;
            while (input_buffer.searchForwards(pos, "\n")) |new_pos| {
                pos = new_pos;
                input_editor.delete(pos, pos + 1);
            }
        }

        // filter completions
        {
            const max_line_string = format(self.app.frame_allocator, "{}", .{target_buffer.countLines()});
            const filter = input_buffer.bytes.items;
            completions_buffer.bytes.shrink(0);
            target_editor.collapseCursors();
            target_editor.clearMark();
            if (action != .None) {
                window.popView();
                window.pushView(self.target_editor_id);
            }
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
                var i: usize = 0;
                while (lines.next()) |line| {
                    if (line.len != 0) {
                        completions_buffer.bytes.appendSlice(line) catch oom();
                        completions_buffer.bytes.append('\n') catch oom();

                        switch (action) {
                            .None, .SelectOne => {
                                if (i + 1 == self.selected) {
                                    var parts = std.mem.split(line, ":");
                                    const path_suffix = parts.next().?;
                                    const line_number_string = parts.next().?;
                                    const line_number = std.fmt.parseInt(usize, line_number_string, 10)
                                        catch |err| panic("{} while parsing line number {s} from rg", .{err, line_number_string});

                                    const path = std.fs.path.join(self.app.frame_allocator, &[2][]const u8{self.project_dir, path_suffix}) catch oom();
                                    target_buffer.load(path);

                                    var cursor = target_editor.getMainCursor();
                                    target_editor.goLine(cursor, line_number - 1);

                                    // TODO centre cursor
                                }
                            },
                        }

                        i += 1;
                    }
                }
            }
        }

        // set selection
        self.selected = min(self.selected, completions_buffer.countLines());
        var cursor = completions_editor.getMainCursor();
        if (self.selected != 0) {
            completions_editor.goPos(cursor, completions_buffer.getPosForLineCol(self.selected - 1, 0));
            completions_editor.setMark();
            completions_editor.goLineEnd(cursor);
        } else {
            completions_editor.clearMark();
            completions_editor.goBufferStart(cursor);
        }

        // run editor frames
        var all_rect = rect;
        const target_rect = all_rect.splitTop(@divTrunc(rect.h, 2), 0);
        const border1_rect = all_rect.splitTop(@divTrunc(self.app.atlas.char_height, 8), 0);
        const input_rect = all_rect.splitBottom(self.app.atlas.char_height, 0);
        const border2_rect = all_rect.splitBottom(@divTrunc(self.app.atlas.char_height, 8), 0);
        const completions_rect = all_rect;
        target_editor.frame(window, target_rect, &[0]c.SDL_Event{});
        window.queueRect(border1_rect, style.text_color);
        completions_editor.frame(window, completions_rect, &[0]c.SDL_Event{});
        window.queueRect(border2_rect, style.text_color);
        input_editor.frame(window, input_rect, input_editor_events.items);
    }
};
