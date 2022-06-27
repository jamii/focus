const std = @import("std");
const glfw = @import("glfw");
const focus = @import("../focus.zig");
const u = focus.util;

pub const Event = union(enum) {
    key_press: struct {
        key: glfw.Key,
        // TODO these fields are not in mach platform
        mods: glfw.Mods,
    },
    key_release: struct {
        key: glfw.Key,
        // TODO these fields are not in mach platform
        mods: glfw.Mods,
    },
    char_input: struct {
        codepoint: u21,
    },
    mouse_motion: struct {
        // These are in window coordinates (not framebuffer coords)
        x: f64,
        y: f64,
    },
    mouse_press: struct {
        button: glfw.mouse_button.MouseButton,
        // These are in window coordinates (not framebuffer coords)
        // TODO these fields are not in mach platform
        pos: glfw.Window.CursorPos,
        mods: glfw.Mods,
    },
    mouse_release: struct {
        button: glfw.mouse_button.MouseButton,
        // These are in window coordinates (not framebuffer coords)
        // TODO these fields are not in mach platform
        pos: glfw.Window.CursorPos,
        mods: glfw.Mods,
    },
    mouse_scroll: struct {
        xoffset: f32,
        yoffset: f32,
    },
    // TODO these events are not in mach platform
    focus_gained,
    focus_lost,
    window_closed,
};

fn keyCallback(window: glfw.Window, key: glfw.Key, scancode: i32, action: glfw.Action, mods: glfw.Mods) void {
    const events = window.getUserPointer(u.ArrayList(Event)) orelse unreachable;
    switch (action) {
        // TODO mach doesn't register repeat
        .press, .repeat => events.append(.{
            .key_press = .{
                .key = key,
                .mods = mods,
            },
        }) catch u.oom(),
        .release => events.append(.{
            .key_release = .{
                .key = key,
                .mods = mods,
            },
        }) catch u.oom(),
    }

    _ = scancode;
    _ = mods;
}

fn mouseMotionCallback(window: glfw.Window, xpos: f64, ypos: f64) void {
    const events = window.getUserPointer(u.ArrayList(Event)) orelse unreachable;
    events.append(.{
        .mouse_motion = .{
            .x = xpos,
            .y = ypos,
        },
    }) catch u.oom();
}

fn mouseButtonCallback(window: glfw.Window, button: glfw.mouse_button.MouseButton, action: glfw.Action, mods: glfw.Mods) void {
    const events = window.getUserPointer(u.ArrayList(Event)) orelse unreachable;
    const cursor_pos = window.getCursorPos() catch |err|
        u.panic("Error getting cursor pos: {}", .{err});
    switch (action) {
        .press => events.append(.{
            .mouse_press = .{
                .button = button,
                .pos = cursor_pos,
                .mods = mods,
            },
        }) catch u.oom(),
        .release => events.append(.{
            .mouse_release = .{
                .button = button,
                .pos = cursor_pos,
                .mods = mods,
            },
        }) catch u.oom(),
        else => {},
    }

    _ = mods;
}

fn scrollCallback(window: glfw.Window, xoffset: f64, yoffset: f64) void {
    const events = (window.getUserPointer(u.ArrayList(Event)) orelse unreachable);
    events.append(.{
        .mouse_scroll = .{
            .xoffset = @floatCast(f32, xoffset),
            .yoffset = @floatCast(f32, yoffset),
        },
    }) catch u.oom();
}

fn focusCallback(window: glfw.Window, focused: bool) void {
    const events = (window.getUserPointer(u.ArrayList(Event)) orelse unreachable);
    events.append(if (focused) .focus_gained else .focus_lost) catch u.oom();
}

fn closeCallback(window: glfw.Window) void {
    const events = (window.getUserPointer(u.ArrayList(Event)) orelse unreachable);
    events.append(.window_closed) catch u.oom();
}

fn charCallback(window: glfw.Window, codepoint: u21) void {
    const events = (window.getUserPointer(u.ArrayList(Event)) orelse unreachable);
    events.append(.{
        .char_input = .{
            .codepoint = codepoint,
        },
    }) catch u.oom();
}

pub fn setCallbacks(window: glfw.Window, events: *u.ArrayList(Event)) void {
    window.setUserPointer(events);
    window.setKeyCallback(keyCallback);
    window.setCursorPosCallback(mouseMotionCallback);
    window.setMouseButtonCallback(mouseButtonCallback);
    window.setScrollCallback(scrollCallback);
    window.setFocusCallback(focusCallback);
    window.setCloseCallback(closeCallback);
    window.setCharCallback(charCallback);
}
