const builtin = @import("builtin");
const std = @import("std");
const Builder = std.build.Builder;
const allocator = std.testing.allocator;
const freetype = @import("mach/libs/freetype/build.zig");
const glfw = @import("mach/libs/glfw/build.zig");

pub fn build(b: *Builder) !void {
    const optimize = b.standardOptimizeOption(.{});
    const target = b.standardTargetOptions(.{});

    const focus_exe = b.addExecutable(.{
        .name = "focus",
        .root_source_file = .{ .path = "./bin/focus.zig" },
        .target = target,
        .optimize = optimize,
    });
    focus_exe.setMainPkgPath("./");
    focus_exe.linkLibC();
    focus_exe.linkSystemLibrary("GL");
    focus_exe.setOutputDir("./zig-cache");
    focus_exe.addModule("freetype", freetype.module(b));
    freetype.link(b, focus_exe, .{});
    focus_exe.addModule("glfw", glfw.module(b));
    try glfw.link(b, focus_exe, .{
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
    focus_exe.omit_frame_pointer = false;
    focus_exe.install();

    const focus_exe_step = b.step("focus", "build your Minimalist text editor");
    focus_exe_step.dependOn(&focus_exe.step);

    const run = focus_exe.run();
    const run_step = b.step("run", "Run focus");
    run_step.dependOn(&run.step);
}
