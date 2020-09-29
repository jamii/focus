const focus = @import("../focus.zig");
usingnamespace focus.common;
const atlas = focus.atlas;
const UI = focus.UI;

pub const Memory = struct {
    allocator: *Allocator,
    data_arena: *ArenaAllocator,
    frame_arena: *ArenaAllocator,
    clozes: []Cloze,
    logs: ArrayList(Log),
    queue: []Cloze.WithState,
    state: State,

    const State = enum {
        Prepare,
        Prompt,
        Reveal,
    };

    pub fn init(allocator: *Allocator) !Memory {
        var data_arena = try allocator.create(ArenaAllocator);
        data_arena.* = ArenaAllocator.init(allocator);
        var frame_arena = try allocator.create(ArenaAllocator);
        frame_arena.* = ArenaAllocator.init(allocator);
        const clozes = try loadClozes(data_arena);
        var logs = ArrayList(Log).init(&data_arena.allocator);
        try logs.appendSlice(try loadLogs(data_arena));
        const queue = try sortByUrgency(data_arena, clozes, logs.items);
        return Memory{
            .allocator = allocator,
            .data_arena = data_arena,
            .frame_arena = frame_arena,
            .clozes = clozes,
            .logs = logs,
            .queue = queue,
            .state = .Prepare,
        };
    }

    pub fn deinit(self: *Memory) void {
        self.data_arena.deinit();
        self.frame_arena.deinit();
        self.allocator.destroy(self.data_arena);
        self.allocator.destroy(self.frame_arena);
    }

    pub fn frame(self: *Memory, ui: *UI, rect: UI.Rect) !void {
        self.frame_arena.deinit();
        self.frame_arena.* = ArenaAllocator.init(self.allocator);
        const allocator = &self.frame_arena.allocator;

        assert(self.queue.len > 0);

        if (ui.key orelse 0 == 'q') {
            try saveLogs(self.logs.items);
            std.os.exit(0);
        }

        const white = UI.Color{ .r = 255, .g = 255, .b = 255, .a = 255 };
        const margin = 5;
        var screen_rect = rect;
        var app_rect = screen_rect.shrink(margin);
        var button_rect = app_rect.splitBottom(UI.buttonHeight(margin), margin);
        var text_rect = app_rect;

        switch (self.state) {
            .Prepare => {
                try ui.text(text_rect, white, try format(allocator, "{} pending", .{self.queue.len}));
                if (try ui.button(button_rect, white, margin, "go")) {
                    self.state = .Prompt;
                }
            },
            .Prompt => {
                const next = self.queue[0];
                try ui.text(text_rect, white, try format(allocator, "{}\n\n(urgency={}, interval={})", .{ next.cloze.renders[next.state.render_ix], next.state.urgency, next.state.interval_ns }));
                if (try ui.button(button_rect, white, margin, "show")) {
                    self.state = .Reveal;
                }
            },
            .Reveal => {
                const next = self.queue[0];
                try ui.text(rect, white, try format(allocator, "{}", .{next.cloze.text}));
                var event_o: ?Log.Event = null;
                var hit_rect = button_rect.splitRight(@divTrunc(button_rect.w, 2), margin);
                var miss_rect = button_rect;
                if (try ui.button(miss_rect, white, margin, "miss")) {
                    event_o = .Miss;
                }
                if (try ui.button(hit_rect, white, margin, "hit")) {
                    event_o = .Hit;
                }
                if (event_o) |event| {
                    try self.logs.append(.{
                        .at_ns = std.time.milliTimestamp() * 1_000_000,
                        .cloze_text = next.cloze.text,
                        .render_ix = next.state.render_ix,
                        .event = event,
                    });
                    self.queue = self.queue[1..];
                    if (self.queue.len == 0) {
                        self.queue = try sortByUrgency(self.frame_arena, self.clozes, self.logs.items);
                        self.state = .Prepare;
                    } else {
                        self.state = .Prompt;
                    }
                }
            },
        }
    }
};

const Cloze = struct {
    filename: []const u8,
    heading: []const u8,
    text: []const u8,
    renders: [][]const u8,

    const State = struct {
        render_ix: usize,
        interval_ns: u64,
        last_hit_ns: u64,
        urgency: f64,
    };

    const WithState = struct {
        cloze: Cloze,
        state: State,
    };
};

const Log = struct {
    at_ns: u64,
    cloze_text: []const u8,
    render_ix: usize,
    event: Event,

    const Event = enum {
        Hit,
        Miss,

        pub fn jsonStringify(self: Event, options: std.json.StringifyOptions, out_stream: anytype) !void {
            try std.fmt.format(out_stream, "\"{}\"", .{@tagName(self)});
        }
    };
};

/// List of clozes written in simple subset of markdown
fn loadClozes(arena: *ArenaAllocator) ![]Cloze {
    const filename = "/home/jamie/exo-secret/memory.md";
    const contents = try std.fs.cwd().readFileAlloc(&arena.allocator, filename, std.math.maxInt(usize));
    var clozes = ArrayList(Cloze).init(&arena.allocator);
    var cloze_strs = std.mem.split(std.fmt.trim(contents), "\n\n");
    var heading: []const u8 = "";
    while (cloze_strs.next()) |cloze_str| {
        if (std.mem.startsWith(u8, cloze_str, "#")) {
            // is a heading
            heading = cloze_str[2..];
        } else if (std.mem.indexOf(u8, cloze_str, "\n")) |line_end| {
            // multi line, blanks are lines 1+
            var render = ArrayList(u8).init(&arena.allocator);
            try render.appendSlice(cloze_str[0..line_end]);
            try render.appendSlice("\n___");
            var renders: [][]const u8 = try arena.allocator.alloc([]const u8, 1);
            renders[0] = render.items;
            try clozes.append(.{
                .filename = filename,
                .heading = heading,
                .text = cloze_str,
                .renders = renders,
            });
        } else {
            // single line, blanks look like *...*
            // even numbers are text, odd numbers are blanks
            var sections = ArrayList([]const u8).init(&arena.allocator);
            var is_code = false;
            var is_blank = false;
            var last_read: usize = 0;
            for (cloze_str) |char, char_ix| {
                switch (char) {
                    '*' => if (!is_code) {
                        try sections.append(cloze_str[last_read..char_ix]);
                        last_read = char_ix + 1; // skip '*'
                        is_blank = !is_blank;
                    },
                    '`' => {
                        is_code = !is_code;
                    },
                    else => {},
                }
            }
            try sections.append(cloze_str[last_read..]);
            assert(is_code == false);
            assert(is_blank == false);
            var text = ArrayList(u8).init(&arena.allocator);
            for (sections.items) |section| {
                try text.appendSlice(section);
            }
            var renders = ArrayList([]const u8).init(&arena.allocator);
            var render_ix: usize = 1;
            while (render_ix < sections.items.len) : (render_ix += 2) {
                var render = ArrayList(u8).init(&arena.allocator);
                for (sections.items) |section, section_ix| {
                    if (section_ix == render_ix) {
                        try render.appendSlice("___");
                    } else {
                        try render.appendSlice(section);
                    }
                }
                try renders.append(render.items);
            }
            try clozes.append(.{
                .filename = filename,
                .heading = heading,
                .text = text.items,
                .renders = renders.items,
            });
        }
    }
    return clozes.items;
}

fn loadLogs(arena: *ArenaAllocator) ![]Log {
    const filename = "/home/jamie/exo-secret/memory.log";
    const contents = try std.fs.cwd().readFileAlloc(&arena.allocator, filename, std.math.maxInt(usize));
    const logs = try std.json.parse([]Log, &std.json.TokenStream.init(contents), std.json.ParseOptions{ .allocator = &arena.allocator });
    return logs;
}

fn saveLogs(logs: []Log) !void {
    const filename = "/home/jamie/exo-secret/memory.log";
    var file = try std.fs.cwd().createFile(filename, .{});
    defer file.close();
    try std.json.stringify(logs, std.json.StringifyOptions{}, file.outStream());
}

fn sortByUrgency(arena: *ArenaAllocator, clozes: []Cloze, logs: []Log) ![]Cloze.WithState {
    const Key = struct {
        cloze_text: []const u8,
        render_ix: usize,
    };
    var states = DeepHashMap(Key, Cloze.WithState).init(&arena.allocator);
    for (clozes) |cloze| {
        for (cloze.renders) |_, render_ix| {
            const key = Key{
                .cloze_text = cloze.text,
                .render_ix = render_ix,
            };
            const value = Cloze.WithState{
                .cloze = cloze,
                .state = Cloze.State{
                    .render_ix = render_ix,
                    .interval_ns = 12 * std.time.hour, // ie 1 day after first hit
                    .last_hit_ns = 0,
                    .urgency = 0,
                },
            };
            try states.putNoClobber(key, value);
        }
    }

    for (logs) |log| {
        const key = Key{
            .cloze_text = log.cloze_text,
            .render_ix = log.render_ix,
        };
        if (states.get(key)) |kv| {
            const state = &kv.value.state;
            switch (log.event) {
                .Hit => {
                    state.interval_ns *= 2;
                    state.last_hit_ns = log.at_ns;
                },
                .Miss => {
                    state.interval_ns /= 2;
                },
            }
        } else {
            warn("Can't find key: {}\n", .{key});
        }
    }
    const now_ns = std.time.milliTimestamp() * 1_000_000;
    var random = std.rand.DefaultPrng.init(42).random;
    var sorted_clozes = ArrayList(Cloze.WithState).init(&arena.allocator);
    var states_iter = states.iterator();
    var new_clozes: isize = 0;
    while (states_iter.next()) |kv| {
        var state = &kv.value.state;
        const since_hit_ns = now_ns - state.last_hit_ns;
        state.urgency = (@intToFloat(f64, since_hit_ns) / @intToFloat(f64, state.interval_ns));
        if (state.last_hit_ns == 0) {
            new_clozes += 1;
            if (new_clozes > 10) {
                continue;
            }
        }
        if (state.urgency < 1) {
            continue;
        }
        // random tiebreaker to avoid always seeing stuff in the same order
        state.urgency *= 1 + ((random.float(f64) - 0.5) / 10);
        try sorted_clozes.append(kv.value);
    }
    std.sort.sort(Cloze.WithState, sorted_clozes.items, moreUrgent);
    return sorted_clozes.items;
}

fn moreUrgent(c0: Cloze.WithState, c1: Cloze.WithState) bool {
    return (c0.state.urgency > c1.state.urgency);
}
