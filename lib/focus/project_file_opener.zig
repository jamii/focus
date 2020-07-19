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

const projects = [6][]const u8{
    "/home/jamie/exo/",
    "/home/jamie/exo-secret/",
    "/home/jamie/imp/",
    "/home/jamie/focus/",
    "/home/jamie/tower/",
    "/home/jamie/zig/",
};

pub const ProjectFileOpener = struct {
    app: *App,
    input_buffer_id: Id,
    input_editor_id: Id,
    selector: Selector,
    paths: []const []const u8,

    pub fn init(app: *App) Id {
        const input_buffer_id = Buffer.initEmpty(app);
        const input_editor_id = Editor.init(app, input_buffer_id);

        const selector = Selector.init(app);

        var paths = ArrayList([]const u8).init(app.allocator);
        for (projects) |project| {
            const result = std.ChildProcess.exec(.{
                .allocator = app.frame_allocator,
                .argv = &[3][]const u8{"rg", "--files", "-0"},
                .cwd = project,
                .max_output_bytes = 128 * 1024 * 1024,
            }) catch |err| panic("{} while calling rg", .{err});
            assert(result.term == .Exited and result.term.Exited == 0);
            var lines = std.mem.split(result.stdout, &[1]u8{0});
            while (lines.next()) |line| {
                const path = std.fs.path.join(app.allocator, &[2][]const u8{project, line}) catch oom();
                paths.append(path) catch oom();
            }
        }

        var self = ProjectFileOpener{
            .app = app,
            .input_buffer_id = input_buffer_id,
            .input_editor_id = input_editor_id,
            .selector = selector,
            .paths = paths.toOwnedSlice(),
        };

        return app.putThing(self);
    }

    pub fn deinit(self: *ProjectFileOpener) void {
        for (self.paths) |completion| {
            self.app.allocator.free(completion);
        }
        self.app.allocator.free(self.paths);
    }

    pub fn frame(self: *ProjectFileOpener, window: *Window, rect: Rect, events: []const c.SDL_Event) void {
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

        // remove any sneaky newlines
        {
            var pos: usize = 0;
            while (input_buffer.searchForwards(pos, "\n")) |new_pos| {
                pos = new_pos;
                input_editor.delete(pos, pos + 1);
            }
        }

        // filter paths
        const ScoredPath = struct {score: ?usize, path: []const u8};
        var scored_paths = ArrayList(ScoredPath).init(self.app.frame_allocator);
        {
            const filter = input_buffer.bytes.items;
            for (self.paths) |path| {
                if (filter.len > 0) {
                    if (std.mem.indexOfScalar(u8, path, filter[0])) |start| {
                        var is_match = true;
                        var end = start;
                        for (filter[1..]) |char| {
                            if (std.mem.indexOfScalarPos(u8, path, end, char)) |new_end| {
                                end = new_end + 1;
                            } else {
                                is_match = false;
                                break;
                            }
                        }
                        if (is_match) {
                            const score = end - start;
                            scored_paths.append(.{.score = score, .path = path}) catch oom();
                        }
                    }
                } else {
                    const score = 0;
                    scored_paths.append(.{.score = score, .path = path}) catch oom();
                }
            }
            std.sort.sort(ScoredPath, scored_paths.items,
                          struct {
                              fn lessThan(a: ScoredPath, b: ScoredPath) bool {
                                  return meta.deepCompare(a,b) == .LessThan;
                              }
                }.lessThan
                          );
        }

        // split rect
        var all_rect = rect;
        const target_rect = all_rect.splitTop(@divTrunc(rect.h, 2), 0);
        const border1_rect = all_rect.splitTop(@divTrunc(self.app.atlas.char_height, 8), 0);
        const input_rect = all_rect.splitBottom(self.app.atlas.char_height, 0);
        const border2_rect = all_rect.splitBottom(@divTrunc(self.app.atlas.char_height, 8), 0);
        const selector_rect = all_rect;
        window.queueRect(border1_rect, style.text_color);
        window.queueRect(border2_rect, style.text_color);

        // run selector frame
        var just_paths = ArrayList([]const u8).init(self.app.frame_allocator);
        for (scored_paths.items) |scored_path| just_paths.append(scored_path.path) catch oom();
        const action = self.selector.frame(window, selector_rect, selector_events.toOwnedSlice(), just_paths.items);

        // maybe open file
        if (action == .SelectOne and just_paths.items.len > 0) {
            const path = just_paths.items[self.selector.selected];
            if (path.len > 0 and std.fs.path.isSep(path[path.len - 1])) {
                input_buffer.bytes.shrink(0);
                input_buffer.bytes.appendSlice(path) catch oom();
                input_editor.goBufferEnd(input_editor.getMainCursor());
            } else {
                const new_buffer_id = Buffer.initFromAbsoluteFilename(self.app, path);
                const new_editor_id = Editor.init(self.app, new_buffer_id);
                window.popView();
                window.pushView(new_editor_id);
            }
        }

        // run other editor frames
        input_editor.frame(window, input_rect, input_events.toOwnedSlice());
    }
};
