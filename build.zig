const Builder = @import("std").build.Builder;

const str = []const u8;

pub fn build(b: *Builder) void {
    const mode = b.standardReleaseOptions();
    const exe = b.addExecutable("focus", "./src/main.zig");
    exe.linkSystemLibrary("c");
    exe.linkSystemLibrary("GL");
    exe.linkSystemLibrary("GLU");
    // exe.linkSystemLibrary("glew");
    exe.linkSystemLibrary("glfw");
    exe.addIncludeDir("./include");
    exe.addCSourceFile("./src/nk_main.c", &[_]str{
        "-std=c99",
        // nuklear causes undefined behaviour
        // https://github.com/ziglang/zig/wiki/FAQ#why-do-i-get-illegal-instruction-when-using-with-zig-cc-to-build-c-code
        "-fno-sanitize=undefined",
    });
    exe.setBuildMode(mode);
    exe.install();

    const run_cmd = exe.run();
    run_cmd.step.dependOn(b.getInstallStep());

    const run_step = b.step("run", "Run the app");
    run_step.dependOn(&run_cmd.step);
}
