const builtin = @import("builtin");
const std = @import("std");
const allocator = std.testing.allocator;
const freetype = @import("mach_freetype");

pub fn build(b: *std.build.Builder) !void {
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
        .main_pkg_path = .{ .path = "./" },
    });
    exe.linkLibC();
    exe.linkSystemLibrary("GL");
    freetypeLink(b, exe);
    glfwLink(b, exe);
    exe.omit_frame_pointer = false;
    exe.addOptions("focus_config", config);
    b.installArtifact(exe);

    const exe_step = b.step("build", "Build");
    exe_step.dependOn(&exe.step);

    const run = b.addRunArtifact(exe);
    const run_step = b.step("run", "Run");
    run_step.dependOn(&run.step);
}

fn freetypeLink(b: *std.Build, step: *std.build.CompileStep) void {
    const mach_freetype_dep = b.dependency("mach_freetype", .{
        .target = step.target,
        .optimize = step.optimize,
    });
    const freetype_dep = b.dependency("mach_freetype.freetype", .{
        .target = step.target,
        .optimize = step.optimize,
    });
    const harfbuzz_dep = b.dependency("mach_freetype.harfbuzz", .{
        .target = step.target,
        .optimize = step.optimize,
    });
    const brotli_dep = b.dependency("mach_freetype.freetype.brotli", .{
        .target = step.target,
        .optimize = step.optimize,
    });

    step.addModule("mach-freetype", mach_freetype_dep.module("mach-freetype"));
    step.addModule("mach-harfbuzz", mach_freetype_dep.module("mach-harfbuzz"));
    step.linkLibrary(freetype_dep.artifact("freetype"));
    step.linkLibrary(harfbuzz_dep.artifact("harfbuzz"));
    step.linkLibrary(brotli_dep.artifact("brotli"));
}

fn glfwLink(b: *std.Build, step: *std.build.CompileStep) void {
    _ = b;
    // TODO Upgrade to zig 0.14 and then use deps/glfw/build.zig directly
    step.addIncludePath(.{ .path = "./deps/glfw/zig-out/include/" });
    step.addObjectFile(.{ .path = "./deps/glfw/zig-out/lib/libglfw.a" });
}
