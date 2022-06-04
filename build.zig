const builtin = @import("builtin");
const std = @import("std");
//const imp2 = @import("imp2/build.zig");
const Builder = std.build.Builder;
const allocator = std.testing.allocator;

pub fn build(b: *Builder) !void {
    const mode = b.standardReleaseOptions();
    var target = b.standardTargetOptions(.{});
    target.setGnuLibCVersion(2, 28, 0);

    const local = b.addExecutable("focus-local", "./bin/focus.zig");
    try includeCommon(local);
    local.setBuildMode(mode);
    local.install();

    const cross = b.addExecutable("focus-cross", "./bin/focus.zig");
    try includeCommon(cross);
    cross.setBuildMode(mode);
    cross.setTarget(std.zig.CrossTarget{
        .cpu_arch = .aarch64,
        .os_tag = .linux,
        .abi = .gnu,
    });
    cross.install();

    const local_step = b.step("local", "Build for local");
    local_step.dependOn(&local.step);

    const cross_step = b.step("cross", "Build for focus");
    cross_step.dependOn(&cross.step);

    const run = local.run();
    const run_step = b.step("run", "Run locally");
    run_step.dependOn(&run.step);
}

fn includeCommon(exe: *std.build.LibExeObjStep) !void {
    exe.setMainPkgPath("./");
    exe.linkSystemLibrary("c");
    exe.linkSystemLibrary("GL");
    exe.linkSystemLibrary("SDL2");
    exe.linkSystemLibrary("SDL2_ttf");
    exe.linkSystemLibrary("pcre2-8");
    try includeNix(exe, "NIX_LIBGL_DEV");
    try includeNix(exe, "NIX_XORGPROTO_DEV");
    try includeNix(exe, "NIX_LIBX11_DEV");
    try includeNix(exe, "NIX_SDL2_DEV");
    try includeNix(exe, "NIX_SDL2_TTF_DEV");
    try includeNix(exe, "NIX_PCRE2_DEV");
    exe.setOutputDir("./zig-cache");
    imp2.addDeps(exe);
}

fn includeNix(exe: *std.build.LibExeObjStep, env_var: []const u8) !void {
    var buf = std.ArrayList(u8).init(allocator);
    defer buf.deinit();
    try buf.appendSlice(std.os.getenv(env_var).?);
    try buf.appendSlice("/include");
    exe.addIncludeDir(buf.items);
}
