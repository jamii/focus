const builtin = @import("builtin");
const std = @import("std");
const allocator = std.testing.allocator;
const freetype = @import("mach_freetype");

pub fn build(b: *std.Build) !void {
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

    const module = b.createModule(.{
        .root_source_file = b.path("./focus.zig"),
        .omit_frame_pointer = false,
        .target = target,
        .optimize = optimize,
        .link_libc = true,
    });
    module.addOptions("focus_config", config);
    module.addImport("mach-freetype", b.dependency("mach_freetype", .{}).module("mach-freetype"));
    module.linkLibrary(b.dependency("glfw", .{}).artifact("glfw"));
    module.linkSystemLibrary("GL", .{});

    const exe = b.addExecutable(.{
        .name = "focus-dev",
        .root_module = module,
    });

    b.installArtifact(exe);

    const exe_step = b.step("build", "Build");
    exe_step.dependOn(&exe.step);

    const run = b.addRunArtifact(exe);
    const run_step = b.step("run", "Run");
    run_step.dependOn(&run.step);
}
