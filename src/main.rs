// Big picture:
//
//   winit          — gives us an OS window and an event loop (no graphics API).
//   glutin         — creates an OpenGL *context* attached to that window. A
//                    GL context is the per-thread state machine that holds all
//                    GL objects (textures, buffers, programs, …) and is the
//                    target of every gl::* call.
//   gl crate       — raw GL function-pointer bindings. They're loaded at runtime
//                    via `gl::load_with` once the context exists.
//   focus::text    — builds a glyph atlas (RGBA texture containing all ASCII
//                    glyphs + a single solid-white texel) and lays out drawing
//                    primitives — text glyphs and solid-color rectangles —
//                    into a Frame's command list.
//   focus::render  — GPU renderer for a Frame's commands. Owns the shader
//                    program, VAO/VBO and atlas texture. Doesn't know
//                    anything about windowing — works the same against a
//                    swap-chain back buffer or an offscreen FBO.
//
// Per-frame drawing recipe (modelled on the Zig editor on master):
//
//   1. one RGBA texture holds every glyph as (255,255,255,alpha) plus a
//      solid-white texel for flat-colored rectangles;
//   2. every frame, the CPU builds a fresh Frame of DrawCommands (Quads
//      interleaved with SetClip commands);
//   3. the renderer flattens that into one vertex buffer + one draw per
//      scissor region.

use std::ffi::CString;
use std::num::NonZeroU32;

use focus::render::Renderer;
use focus::text::{Atlas, Font, FontSettings, Frame, Rect};
use glutin::config::ConfigTemplateBuilder;
use glutin::context::{ContextApi, ContextAttributesBuilder, PossiblyCurrentContext, Version};
use glutin::display::GetGlDisplay;
use glutin::prelude::*;
use glutin::surface::{Surface, SwapInterval, WindowSurface};
use glutin_winit::{DisplayBuilder, GlWindow};
use raw_window_handle::HasWindowHandle;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::platform::wayland::WindowAttributesExtWayland;
use winit::window::{Window, WindowId};

// Use a distinct title + app_id in debug builds so a niri window-rule can
// match only the dev instance (e.g. `open-focused false`).
const APP_ID: &str = if cfg!(debug_assertions) {
    "focus-debug"
} else {
    "focus"
};

const TEXT: &str = "hello world";
const TEXT_X: f32 = 20.0;
const TEXT_Y: f32 = 20.0;
const INITIAL_PX: f32 = 32.0;
const MIN_PX: f32 = 4.0;

const TEXT_COLOR: [u8; 4] = [30, 30, 40, 255];
const HIGHLIGHT_COLOR: [u8; 4] = [255, 240, 170, 255];

struct State {
    window: Window,
    surface: Surface<WindowSurface>,
    context: PossiblyCurrentContext,
    renderer: Renderer,
    font: Font,
    px_size: f32,
    atlas: Atlas,
}

impl State {
    // ControlFlow::Wait means no automatic redraw — request one.
    fn rebuild_atlas(&mut self) {
        self.atlas = Atlas::build(&self.font, self.px_size);
        unsafe { self.renderer.upload_atlas(&self.atlas) };
        self.window.request_redraw();
    }

    // Build this frame's commands, hand them to the renderer, present.
    fn draw(&mut self) {
        let size = self.window.inner_size();
        let fb_w = size.width as f32;
        let fb_h = size.height as f32;

        let mut frame = Frame::new(fb_w, fb_h);

        // A clip rect deliberately narrower than the text — the right side
        // of "hello world" will get scissored off.
        let clip = Rect::new(
            TEXT_X - 4.0,
            TEXT_Y - 4.0,
            180.0,
            self.atlas.line_height + 8.0,
        );
        frame.push_clip_rect(clip);
        // The highlight is the clip box itself — software-trimmed inside
        // draw_rect, so it never overflows.
        frame.draw_rect(&self.atlas, clip, HIGHLIGHT_COLOR);
        // Text overflows the right edge of the clip; the SetClip / scissor
        // pair around it cuts the trailing glyphs at their pixel edges.
        frame.draw_text(&self.atlas, TEXT, TEXT_X, TEXT_Y, TEXT_COLOR);
        frame.pop_clip_rect();

        unsafe {
            self.renderer.render(
                frame.commands(),
                &self.atlas,
                size.width as i32,
                size.height as i32,
            );
        }
        self.surface.swap_buffers(&self.context).unwrap();
    }
}

#[derive(Default)]
struct App {
    state: Option<State>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }

        let window_attrs = Window::default_attributes()
            .with_title(APP_ID)
            .with_name(APP_ID, "")
            .with_inner_size(LogicalSize::new(800, 600));

        let template = ConfigTemplateBuilder::new().with_alpha_size(8);
        let display_builder = DisplayBuilder::new().with_window_attributes(Some(window_attrs));

        let (window, gl_config) = display_builder
            .build(event_loop, template, |mut configs| configs.next().unwrap())
            .unwrap();
        let window = window.expect("winit window not created");

        let raw_handle = window.window_handle().ok().map(|h| h.as_raw());
        let gl_display = gl_config.display();

        let context_attrs = ContextAttributesBuilder::new()
            .with_context_api(ContextApi::OpenGl(Some(Version::new(3, 3))))
            .build(raw_handle);

        let not_current = unsafe {
            gl_display
                .create_context(&gl_config, &context_attrs)
                .expect("failed to create GL context")
        };

        let surface_attrs = window.build_surface_attributes(Default::default()).unwrap();
        let surface = unsafe {
            gl_display
                .create_window_surface(&gl_config, &surface_attrs)
                .expect("failed to create surface")
        };

        let context = not_current
            .make_current(&surface)
            .expect("failed to make context current");

        let _ =
            surface.set_swap_interval(&context, SwapInterval::Wait(NonZeroU32::new(1).unwrap()));

        gl::load_with(|s| {
            let cstr = CString::new(s).unwrap();
            gl_display.get_proc_address(&cstr) as *const _
        });

        let renderer = unsafe { Renderer::new() };

        let font_bytes = std::fs::read("deps/FiraCode-Regular.ttf").unwrap();
        let font = Font::from_bytes(font_bytes, FontSettings::default()).unwrap();
        let atlas = Atlas::build(&font, INITIAL_PX);
        unsafe { renderer.upload_atlas(&atlas) };

        let state = State {
            window,
            surface,
            context,
            renderer,
            font,
            px_size: INITIAL_PX,
            atlas,
        };
        state.window.request_redraw();
        self.state = Some(state);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(state) = self.state.as_mut() else {
            return;
        };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key,
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => match logical_key.as_ref() {
                Key::Named(NamedKey::Escape) => event_loop.exit(),
                Key::Character("+") => {
                    state.px_size += 1.0;
                    state.rebuild_atlas();
                }
                Key::Character("-") => {
                    state.px_size = (state.px_size - 1.0).max(MIN_PX);
                    state.rebuild_atlas();
                }
                _ => {}
            },
            WindowEvent::Resized(size) => {
                if let (Some(w), Some(h)) =
                    (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
                {
                    state.surface.resize(&state.context, w, h);
                }
            }
            WindowEvent::RedrawRequested => state.draw(),
            _ => {}
        }
    }
}

fn main() {
    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = App::default();
    event_loop.run_app(&mut app).unwrap();
}
