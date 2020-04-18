usingnamespace @import("./common.zig");

const str = []const u8;

const Cloze = struct {
    filename: str,
    heading: str,
    text: str,
    renders: []str,
};

/// List of clozes written in simple subset of markdown
pub fn parse(arena: *ArenaAllocator) ![]Cloze {
    const filename = "/home/jamie/exo-secret/memory.md";
    var heading: str = "";
    var clozes = ArrayList(Cloze).init(&arena.allocator);
    const contents = try std.fs.cwd().readFileAlloc(&arena.allocator, filename, std.math.maxInt(usize));
    var cloze_strs = std.mem.split(std.fmt.trim(contents), "\n\n");
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
