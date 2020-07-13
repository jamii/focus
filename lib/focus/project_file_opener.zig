const focus = @import("../focus.zig");
usingnamespace focus.common;
const meta = focus.meta;
const App = focus.App;
const Id = focus.Id;
const Buffer = focus.Buffer;
const Editor = focus.Editor;
const Window = focus.Window;
const style = focus.style;

const projects = [6][]const u8{
    "/home/jamie/exo/",
    "/home/jamie/exo-secret/",
    "/home/jamie/imp/",
    "/home/jamie/focus/",
    "/home/jamie/tower/",
    "/home/jamie/zig/",
};

// TODO arena allocator for lifespan of opener

// TODO always have a selection, open first match on enter (reject if no match), open raw text on ctrl-enter

pub const ProjectFileOpener = struct {
    app: *App,
    input_buffer_id: Id,
    input_editor_id: Id,
    completions_buffer_id: Id,
    completions_editor_id: Id,
    selected: usize, // 0 for nothing selected, i-1 for line i
    completions: []const []const u8,

    pub fn init(app: *App) !Id {
        // TODO don't directly mutate buffer - messes up multiple cursors - go via editor instead
        const input_buffer_id = try Buffer.initEmpty(app);
        const input_editor_id = try Editor.init(app, input_buffer_id);
        const completions_buffer_id = try Buffer.initEmpty(app);
        const completions_editor_id = try Editor.init(app, completions_buffer_id);

        var completions = ArrayList([]const u8).init(app.allocator);
        for (projects) |project| {
            const result = try std.ChildProcess.exec(.{
                .allocator = app.frame_allocator,
                .argv = &[3][]const u8{"rg", "--files", "-0"},
                .cwd = project,
                .max_output_bytes = 128 * 1024 * 1024,
            });
            assert(result.term == .Exited and result.term.Exited == 0);
            var lines = std.mem.split(result.stdout, &[1]u8{0});
            while (lines.next()) |line| {
                const completion = try std.fs.path.join(app.allocator, &[2][]const u8{project, line});
                try completions.append(completion);
            }
        }

        var self = ProjectFileOpener{
            .app = app,
            .input_buffer_id = input_buffer_id,
            .input_editor_id = input_editor_id,
            .completions_buffer_id = completions_buffer_id,
            .completions_editor_id = completions_editor_id,
            .selected = 0,
            .completions = completions.toOwnedSlice(),
        };

        return app.putThing(self);
    }

    pub fn deinit(self: *ProjectFileOpener) void {
        for (self.completions) |completion| {
            self.app.allocator.free(completion);
        }
        self.app.allocator.free(self.completions);
    }

    pub fn frame(self: *ProjectFileOpener, window: *Window, rect: Rect, events: []const c.SDL_Event) !void {
        var input_buffer = self.app.getThing(self.input_buffer_id).Buffer;
        var input_editor = self.app.getThing(self.input_editor_id).Editor;
        var completions_buffer = self.app.getThing(self.completions_buffer_id).Buffer;
        var completions_editor = self.app.getThing(self.completions_editor_id).Editor;

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
                            else => delegate = true,
                        }
                    } else if (sym.mod == 0) {
                        switch (sym.sym) {
                            c.SDLK_RETURN => {
                                var filename = ArrayList(u8).init(self.app.frame_allocator);
                                if (self.selected == 0) {
                                    try filename.appendSlice(input_buffer.bytes.items);
                                } else {
                                    const selection = try completions_editor.dupeSelection(self.app.frame_allocator, completions_editor.getMainCursor());
                                    try filename.appendSlice(selection);
                                }
                                if (filename.items.len > 0 and std.fs.path.isSep(filename.items[filename.items.len - 1])) {
                                    input_buffer.bytes.shrink(0);
                                    try input_buffer.bytes.appendSlice(filename.items);
                                    input_editor.goBufferEnd(input_editor.getMainCursor());
                                    filename.deinit();
                                } else {
                                    const new_buffer_id = try Buffer.initFromAbsoluteFilename(self.app, filename.toOwnedSlice());
                                    const new_editor_id = try Editor.init(self.app, new_buffer_id);
                                    window.popView();
                                    try window.pushView(new_editor_id);
                                }
                            },
                            c.SDLK_TAB => {
                                var min_common_prefix_o: ?[]const u8 = null;
                                var lines_iter = std.mem.split(completions_buffer.bytes.items, "\n");
                                while (lines_iter.next()) |line| {
                                    if (line.len != 0) {
                                        if (min_common_prefix_o) |min_common_prefix| {
                                            var i: usize = 0;
                                            while (i < min(min_common_prefix.len, line.len) and min_common_prefix[i] == line[i]) i += 1;
                                            min_common_prefix_o = line[0..i];
                                        } else {
                                            min_common_prefix_o = line;
                                        }
                                    }
                                }
                                if (min_common_prefix_o) |min_common_prefix| {
                                    const path = input_buffer.bytes.items;
                                    input_buffer.delete(0, input_buffer.getBufferEnd());
                                    try input_buffer.insert(input_buffer.getBufferEnd(), min_common_prefix);
                                    input_editor.goPos(input_editor.getMainCursor(), input_buffer.getBufferEnd());
                                }
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
            if (delegate) try input_editor_events.append(event);
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
            const filter = input_buffer.bytes.items;
            const ScoredCompletion = struct {score: ?usize, completion: []const u8};
            var scored_completions = ArrayList(ScoredCompletion).init(self.app.frame_allocator);
            for (self.completions) |completion| {
                if (filter.len > 0) {
                    if (std.mem.indexOfScalar(u8, completion, filter[0])) |start| {
                        var is_match = true;
                        var end = start;
                        for (filter[1..]) |char| {
                            if (std.mem.indexOfScalarPos(u8, completion, end, char)) |new_end| {
                                end = new_end;
                            } else {
                                is_match = false;
                                break;
                            }
                        }
                        if (is_match) {
                            const score = end - start;
                            try scored_completions.append(.{.score = score, .completion = completion});
                        }
                    }
                } else {
                    const score = 0;
                    try scored_completions.append(.{.score = score, .completion = completion});
                }
            }
            std.sort.sort(ScoredCompletion, scored_completions.items,
                          struct {
                              fn lessThan(a: ScoredCompletion, b: ScoredCompletion) bool {
                                  return meta.deepCompare(a,b) == .LessThan;
                              }
                }.lessThan
                          );
            completions_buffer.bytes.shrink(0);
            for (scored_completions.items) |scored_completion| {
                try completions_buffer.bytes.appendSlice(scored_completion.completion);
                try completions_buffer.bytes.append('\n');
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
        var completions_rect = rect;
        const input_rect = completions_rect.splitTop(self.app.atlas.char_height, 0);
        const border_rect = completions_rect.splitTop(@divTrunc(self.app.atlas.char_height, 8), 0);
        try input_editor.frame(window, input_rect, input_editor_events.items);
        try window.queueRect(border_rect, style.text_color);
        try completions_editor.frame(window, completions_rect, &[0]c.SDL_Event{});
    }
};
