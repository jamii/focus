const builtin = @import("builtin");
const std = @import("std");
const Builder = std.build.Builder;
const allocator = std.testing.allocator;
const freetype = @import("mach/freetype/build.zig");
const glfw = @import("mach/glfw/build.zig");

pub fn build(b: *Builder) !void {
    const mode = b.standardReleaseOptions();
    const target = b.standardTargetOptions(.{});

    const local = b.addExecutable("focus-local", "./bin/focus.zig");
    try includeCommon(b, local);
    local.setBuildMode(mode);
    local.setTarget(target);
    local.install();

    const cross = b.addExecutable("focus-cross", "./bin/focus.zig");
    try includeCommon(b, cross);
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

fn includeCommon(b: *Builder, exe: *std.build.LibExeObjStep) !void {
    exe.setMainPkgPath("./");
    exe.linkSystemLibrary("c");
    exe.linkSystemLibrary("GL");
    exe.linkSystemLibrary("pcre2-8");
    exe.setOutputDir("./zig-cache");
    exe.addPackage(freetype.pkg);
    freetype.link(b, exe, .{});
    exe.addPackage(glfw.pkg);
    glfw.link(b, exe, .{
        .vulkan = false,
        .metal = false,
        .opengl = true,
        .gles = false,
        .x11 = true,
        // TODO try wayland
        // https://github.com/hexops/mach/issues/347
        .wayland = false,
        .system_sdk = .{},
    });
    exe.omit_frame_pointer = false;
}
