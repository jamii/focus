usingnamespace @import("./common.zig");

pub const atlas = @import("./atlas.zig");
pub const Fui = @import("./fui.zig").Fui;

pub const Memory = struct {
    allocator: *Allocator,
    data_arena: ArenaAllocator,
    frame_arena: ArenaAllocator,
    clozes: []Cloze,
    logs: ArrayList(Log),
    urgency_threshold: f64,
    queue: []Cloze.WithState,
    state: State,

    const State = enum {
        Prepare,
        Prompt,
        Reveal,
    };

    pub fn init(allocator: *Allocator) !Memory {
        var data_arena = ArenaAllocator.init(allocator);
        const frame_arena = ArenaAllocator.init(allocator);
        const clozes = try loadClozes(&data_arena);
        // TODO we consistently get segfaults if we use &data_arena.allocator here - why?
        var logs = ArrayList(Log).init(allocator);
        try logs.appendSlice(try loadLogs(&data_arena));
        const queue = try sortByUrgency(&data_arena, clozes, logs.items);
        return Memory{
            .allocator = allocator,
            .data_arena = data_arena,
            .frame_arena = frame_arena,
            .clozes = clozes,
            .logs = logs,
            .urgency_threshold = 1.0,
            .queue = queue,
            .state = .Prepare,
        };
    }

    pub fn deinit(self: *Memory) void {
        self.data_arena.deinit();
        self.frame_arena.deinit();
        self.logs.deinit();
    }

    pub fn frame(self: *Memory, fui: *Fui, rect: Fui.Rect) !void {
        // TODO remove `orelse 0`
        // https://github.com/ziglang/zig/issues/1332
        self.frame_arena.deinit();
        self.frame_arena = ArenaAllocator.init(self.allocator);
        const allocator = &self.frame_arena.allocator;
        assert(self.queue.len > 0);
        if (fui.key orelse 0 == 'q') {
            try saveLogs(self.logs.items);
            std.os.exit(0);
        }
        switch (self.state) {
            .Prepare => {
                try fui.text(rect, try format(allocator, "{} pending", .{self.queue.len}), .{.r=255, .g=255, .b=255, .a=255});
                if (try fui.button(.{.x=0, .y=rect.h-atlas.text_height, .w=rect.w, .h=atlas.text_height}, "go", .{.r=255, .g=255, .b=255, .a=255})) {
                    self.state = .Prompt;
                }
            },
            .Prompt => {
                const next = self.queue[0];
                try fui.text(rect, try format(allocator, "{}\n\n(urgency={}, interval={})", .{next.cloze.renders[next.state.render_ix], next.state.urgency, next.state.interval_ns}), .{.r=255, .g=255, .b=255, .a=255});
                if (try fui.button(.{.x=0, .y=rect.h-atlas.text_height, .w=rect.w, .h=atlas.text_height}, "show", .{.r=255, .g=255, .b=255, .a=255})) {
                    self.state = .Reveal;
                }
            },
            .Reveal => reveal: {
                const next = self.queue[0];
                try fui.text(rect, try format(allocator, "{}\n", .{next.cloze.text}), .{.r=255, .g=255, .b=255, .a=255});
                var event_o: ?Log.Event = null;
                if (try fui.button(.{.x=0, .y=rect.h-atlas.text_height, .w=@divTrunc(rect.w, 2), .h=atlas.text_height}, "miss", .{.r=255, .g=255, .b=255, .a=255})) {
                    event_o = .Miss;
                }
                if (try fui.button(.{.x=@divTrunc(rect.w, 2), .y=rect.h-atlas.text_height, .w=@divTrunc(rect.w, 2), .h=atlas.text_height}, "hit", .{.r=255, .g=255, .b=255, .a=255})) {
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
                        self.queue = try sortByUrgency(&self.frame_arena, self.clozes, self.logs.items);
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
    filename: str,
    heading: str,
    text: str,
    renders: []str,

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
    cloze_text: str,
    render_ix: usize,
    event: Event,

    const Event = enum {
        Hit,
        Miss,

        pub fn jsonStringify(self: Event, options: std.json.StringifyOptions, out_stream: var) !void {
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
    var heading: str = "";
    while (cloze_strs.next()) |cloze_str| {
        if (std.mem.startsWith(u8, cloze_str, "#")) {
            // is a heading
            heading = cloze_str[2..];
        } else if (std.mem.indexOf(u8, cloze_str, "\n")) |line_end| {
            // multi line, blanks are lines 1+
            var render = ArrayList(u8).init(&arena.allocator);
            try render.appendSlice(cloze_str[0..line_end]);
            try render.appendSlice("\n___");
            var renders: []str = try arena.allocator.alloc(str, 1);
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
            var sections = ArrayList(str).init(&arena.allocator);
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
                    else => {}
                }
            }
            assert(is_code == false);
            assert(is_blank == false);
            var renders = ArrayList(str).init(&arena.allocator);
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
            try sections.append(cloze_str[last_read..]);
            try clozes.append(.{
                .filename = filename,
                .heading = heading,
                .text = cloze_str,
                .renders = renders.items,
            });
        }
    }
    return clozes.items;
}

fn loadLogs(arena: *ArenaAllocator) ![]Log {
    const filename = "/home/jamie/exo-secret/memory.log";
    const contents = try std.fs.cwd().readFileAlloc(&arena.allocator, filename, std.math.maxInt(usize));
    const logs = try std.json.parse(
        []Log,
        &std.json.TokenStream.init(contents),
        std.json.ParseOptions{.allocator = &arena.allocator}
    );
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
        cloze_text: str,
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
                    // newly added stuff is at back of queue until all urgent stuff is done
                    .interval_ns = 24 * std.time.hour,
                    .last_hit_ns = std.time.milliTimestamp() * 1_000_000,
                    .urgency = 0,
                }
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
                }
            }
        } else {
            warn("Can't find key: {}\n", .{key});
        }
    }
    const now_ns = std.time.milliTimestamp() * 1_000_000;
    var random = std.rand.DefaultPrng.init(42).random;
    var sorted_clozes = ArrayList(Cloze.WithState).init(&arena.allocator);
    var states_iter = states.iterator();
    while (states_iter.next()) |kv| {
        var state = &kv.value.state;
        const since_hit_ns = now_ns - state.last_hit_ns;
        // random tiebreaker on urgency to avoid always seeing stuff in the same order
        const tiebreaker: f64 = 1 + ((random.float(f64) - 0.5) / 10);
        state.urgency = tiebreaker * (@intToFloat(f64, since_hit_ns) / @intToFloat(f64, state.interval_ns));
        try sorted_clozes.append(kv.value);
    }
    std.sort.sort(Cloze.WithState, sorted_clozes.items, moreUrgent);
    return sorted_clozes.items;
}

fn moreUrgent(c0: Cloze.WithState, c1: Cloze.WithState) bool {
    return (c0.state.urgency > c1.state.urgency);
}
