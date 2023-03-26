const builtin = @import("builtin");
const std = @import("std");
const Builder = std.build.Builder;
const allocator = std.testing.allocator;
const freetype = @import("mach/libs/freetype/build.zig");
const glfw = @import("mach/libs/glfw/build.zig");

pub fn build(b: *Builder) !void {
    const optimize = b.standardOptimizeOption(.{});
    const target = b.standardTargetOptions(.{});

    const home_path = std.os.getenv("HOME") orelse @panic("HOME environment variable is not defined");
    const focus_state_dir = fsd: {
        var focus_state_dir_path: ?[]const u8 = null;
        // $XDG_STATE_HOME defines the base directory relative to which user-specific state files should be stored
        if (std.os.getenv("XDG_STATE_HOME")) |xdg_state_home| {
            if (xdg_state_home.len != 0) {
                focus_state_dir_path = try std.fmt.allocPrint(b.allocator, "{s}/focus", .{xdg_state_home});
            }
        }
        // If $XDG_STATE_HOME is either not set or empty, a default equal to $HOME/.local/state should be used
        if (focus_state_dir_path == null) {
            focus_state_dir_path = try std.fmt.allocPrint(b.allocator, "{s}/.local/state/focus", .{home_path});
        }
        break :fsd try std.fs.cwd().makeOpenPath(focus_state_dir_path.?, .{});
    };
    const projects_file_path = pfp: {
        const projects_file_path = try std.fmt.allocPrint(b.allocator, "{s}/projects", .{try focus_state_dir.realpathAlloc(b.allocator, ".")});
        var projects_file = focus_state_dir.createFile(projects_file_path, .{.truncate = false, .exclusive = true}) catch |err| switch (err) {
            error.PathAlreadyExists => {
                break :pfp projects_file_path;
            },
            else => return err,
        };
        _ = try projects_file.write(try std.fs.cwd().realpathAlloc(b.allocator, "."));
        projects_file.close();
        break :pfp projects_file_path;
    };
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
    const focus_paths = b.addOptions();
    focus_paths.addOption([]const u8, "home_path", home_path);
    focus_paths.addOption([]const u8, "projects_file_path", projects_file_path);
    focus_exe.addOptions("focus_paths", focus_paths);
    focus_exe.install();

    const focus_exe_step = b.step("focus", "build your Minimalist text editor");
    focus_exe_step.dependOn(&focus_exe.step);

    const run = focus_exe.run();
    const run_step = b.step("run", "Run focus");
    run_step.dependOn(&run.step);
}
