const builtin = @import("builtin");
const std = @import("std");
const Builder = std.build.Builder;
const allocator = std.testing.allocator;
const freetype = @import("mach/libs/freetype/build.zig");
const glfw = @import("mach/libs/glfw/build.zig");

pub fn build(b: *Builder) !void {
    const optimize = b.standardOptimizeOption(.{});
    const target = b.standardTargetOptions(.{});

    const config = b.addOptions();
    config.addOption(
        []const u8,
        "home_path",
        b.option([]const u8, "home-path", "") orelse "/home/jamie/",
    );
    config.addOption(
        []const u8,
        "projects_file_path",
        b.option([]const u8, "projects-file-path", "") orelse "/home/jamie/secret/projects",
    );

    const exe = b.addExecutable(.{
        .name = "focus-dev",
        .root_source_file = .{ .path = "./bin/focus.zig" },
        .target = target,
        .optimize = optimize,
    });
    exe.setMainPkgPath("./");
    exe.linkLibC();
    exe.linkSystemLibrary("GL");
    exe.setOutputDir("./zig-cache");
    exe.addModule("freetype", freetype.module(b));
    freetype.link(b, exe, .{});
    exe.addModule("glfw", glfw.module(b));
    try glfw.link(b, exe, .{
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
    exe.addOptions("focus_config", config);
    exe.install();

    const exe_step = b.step("build", "Build");
    exe_step.dependOn(&exe.step);

    const run = exe.run();
    const run_step = b.step("run", "Run");
    run_step.dependOn(&run.step);
}
