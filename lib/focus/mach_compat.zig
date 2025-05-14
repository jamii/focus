const std = @import("std");
const focus = @import("../focus.zig");
const u = focus.util;
const c = focus.util.c;

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
        xoffset: f64,
        yoffset: f64,
    },
    focus_gained,
    focus_lost,
    window_closed,
};

pub const KeyEvent = struct {
    key: c_int,
    mods: c_int,
};

pub const MouseButtonEvent = struct {
    button: c_int,
    pos: [2]f64,
    mods: c_int,
};

fn getEventsList(window: ?*c.GLFWwindow) *u.ArrayList(Event) {
    return @ptrCast(@alignCast(c.glfwGetWindowUserPointer(window)));
}

fn keyCallback(window: ?*c.GLFWwindow, key: c_int, scancode: c_int, action: c_int, mods: c_int) callconv(.C) void {
    const events = getEventsList(window);
    const key_event = KeyEvent{
        .key = key,
        .mods = mods,
    };
    const event = switch (action) {
        c.GLFW_PRESS => Event{ .key_press = key_event },
        c.GLFW_REPEAT => Event{ .key_repeat = key_event },
        c.GLFW_RELEASE => Event{ .key_release = key_event },
        else => u.panic("Unexpected action: {}", .{action}),
    };
    events.append(event) catch u.oom();
    _ = scancode;
}

fn mouseMotionCallback(window: ?*c.GLFWwindow, xpos: f64, ypos: f64) callconv(.C) void {
    const events = getEventsList(window);
    events.append(.{
        .mouse_motion = .{
            .x = xpos,
            .y = ypos,
        },
    }) catch u.oom();
}

fn mouseButtonCallback(window: ?*c.GLFWwindow, button: c_int, action: c_int, mods: c_int) callconv(.C) void {
    const events = getEventsList(window);
    var cursor_pos: [2]f64 = .{ 0, 0 };
    c.glfwGetCursorPos(window, &cursor_pos[0], &cursor_pos[1]);
    const mouse_button_event = MouseButtonEvent{
        .button = button,
        .pos = cursor_pos,
        .mods = mods,
    };
    const event = switch (action) {
        c.GLFW_PRESS => Event{ .mouse_press = mouse_button_event },
        c.GLFW_RELEASE => Event{ .mouse_release = mouse_button_event },
        else => u.panic("Unexpected action: {}", .{action}),
    };
    events.append(event) catch u.oom();
}

fn scrollCallback(window: ?*c.GLFWwindow, xoffset: f64, yoffset: f64) callconv(.C) void {
    const events = getEventsList(window);
    events.append(.{
        .mouse_scroll = .{
            .xoffset = xoffset,
            .yoffset = yoffset,
        },
    }) catch u.oom();
}

fn focusCallback(window: ?*c.GLFWwindow, focused: c_int) callconv(.C) void {
    const events = getEventsList(window);
    events.append(if (focused == c.GLFW_TRUE) .focus_gained else .focus_lost) catch u.oom();
}

fn closeCallback(window: ?*c.GLFWwindow) callconv(.C) void {
    const events = getEventsList(window);
    events.append(.window_closed) catch u.oom();
}

fn charCallback(window: ?*c.GLFWwindow, codepoint: c_uint) callconv(.C) void {
    const events = getEventsList(window);
    events.append(.{
        .char_input = .{
            .codepoint = @intCast(codepoint),
        },
    }) catch u.oom();
}

pub fn setCallbacks(window: ?*c.GLFWwindow, events: *u.ArrayList(Event)) callconv(.C) void {
    _ = c.glfwSetWindowUserPointer(window, events);
    _ = c.glfwSetKeyCallback(window, keyCallback);
    _ = c.glfwSetCursorPosCallback(window, mouseMotionCallback);
    _ = c.glfwSetMouseButtonCallback(window, mouseButtonCallback);
    _ = c.glfwSetScrollCallback(window, scrollCallback);
    _ = c.glfwSetWindowFocusCallback(window, focusCallback);
    _ = c.glfwSetWindowCloseCallback(window, closeCallback);
    _ = c.glfwSetCharCallback(window, charCallback);
}
