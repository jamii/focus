// defining stuff in root can be dangerous

pub fn main() anyerror!void {
    try @import("./main.zig").main();
}
