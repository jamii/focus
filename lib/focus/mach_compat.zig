const std = @import("std");
const glfw = @import("glfw");
const focus = @import("../focus.zig");
const u = focus.util;

// TODO switch to mach.platform when stable

pub const Event = union(enum) {
    key_press: KeyEvent,
    key_repeat: KeyEvent,
    key_release: KeyEvent,
    char_input: struct {
        codepoint: u21,
    },
    mouse_motion: struct {
        x: f64,
        y: f64,
    },
    mouse_press: MouseButtonEvent,
    mouse_release: MouseButtonEvent,
    mouse_scroll: struct {
        xoffset: f32,
        yoffset: f32,
    },
    focus_gained,
    focus_lost,
    window_closed,
};

pub const KeyEvent = struct {
    key: glfw.Key,
    mods: glfw.Mods,
};

pub const MouseButtonEvent = struct {
    button: glfw.mouse_button.MouseButton,
    pos: glfw.Window.CursorPos,
    mods: glfw.Mods,
};

fn keyCallback(window: glfw.Window, key: glfw.Key, scancode: i32, action: glfw.Action, mods: glfw.Mods) void {
    const events = window.getUserPointer(u.ArrayList(Event)) orelse unreachable;
    const key_event = KeyEvent{
        .key = key,
        .mods = mods,
    };
    const event = switch (action) {
        .press => Event{ .key_press = key_event },
        .repeat => Event{ .key_repeat = key_event },
        .release => Event{ .key_release = key_event },
    };
    events.append(event) catch u.oom();
    _ = scancode;
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
    const cursor_pos = window.getCursorPos();
    const mouse_button_event = MouseButtonEvent{
        .button = button,
        .pos = cursor_pos,
        .mods = mods,
    };
    switch (action) {
        .press => events.append(.{ .mouse_press = mouse_button_event }) catch u.oom(),
        .release => events.append(.{ .mouse_release = mouse_button_event }) catch u.oom(),
        else => {},
    }
}

fn scrollCallback(window: glfw.Window, xoffset: f64, yoffset: f64) void {
    const events = (window.getUserPointer(u.ArrayList(Event)) orelse unreachable);
    events.append(.{
        .mouse_scroll = .{
            .xoffset = @floatCast(xoffset),
            .yoffset = @floatCast(yoffset),
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
