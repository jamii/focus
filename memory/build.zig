const std = @import("std");
const Builder = std.build.Builder;
const builtin = @import("builtin");

const str = []const u8;
const strbuf = std.ArrayList(u8);
const alloc = std.testing.allocator;

pub fn build(b: *Builder) !void {

    const mode = b.standardReleaseOptions();

    const local = b.addExecutable("focus", "./src/main.zig");
    include_common(local);
    local.setBuildMode(mode);
    local.install();

    const cross = b.addExecutable("focus-cross", "./src/main.zig");
    include_common(cross);
    cross.setBuildMode(mode);
    cross.setTarget(
        std.zig.CrossTarget{
            .cpu_arch = .aarch64,
            .os_tag = .linux,
            .abi = .gnu,
        }
    );
    cross.install();

    const local_step = b.step("local", "Build for local");
    local_step.dependOn(&local.step);

    const cross_step = b.step("cross", "Build for focus");
    cross_step.dependOn(&cross.step);

    const run = local.run();
    const run_step = b.step("run", "Run locally");
    run_step.dependOn(&run.step);
}

fn include_common(exe: *std.build.LibExeObjStep) void {
    exe.linkSystemLibrary("c");
    exe.linkSystemLibrary("GL");
    exe.linkSystemLibrary("GLU");
    exe.linkSystemLibrary("glfw3");
    exe.addIncludeDir("./include");
    exe.addCSourceFile("./src/nk_main.c", &[_]str{
        "-std=c99",
        // nuklear causes undefined behaviour
        // https://github.com/ziglang/zig/wiki/FAQ#why-do-i-get-illegal-instruction-when-using-with-zig-cc-to-build-c-code
        "-fno-sanitize=undefined",
    });
    exe.setOutputDir("zig-cache");
}

fn include_nix(exe: *std.build.LibExeObjStep, env_var: str) !void {
    var buf = strbuf.init(alloc);
    defer buf.deinit();
    try buf.appendSlice(std.os.getenv(env_var).?);
    try buf.appendSlice("/include");
    exe.addIncludeDir(buf.items);
}
