// Plumbing layer between the OS / OpenGL and `focus::app`.
//
//   winit    — OS windows + event loop.
//   glutin   — OpenGL contexts attached to those windows.
//   gl crate — raw GL function pointers.
//   app::App
//            — the editor. Receives `Input`s (which wrap winit
//              `WindowEvent`s) via `App::input`, drives external effects
//              through the `IO` trait, and draws into a `Drawing` (a
//              command list of quads + clip rects).
//
// The loop:
//   1. winit hands us events; we wrap each into an `Input`.
//   2. We `pump`: build an `Outside` (the live `IO` impl), drain inputs
//      into `App::input`, then `App::tick`. The IO methods directly
//      open/close windows, re-upload the atlas, or exit.
//   3. On `about_to_wait`, build a `Drawing` per window and hand the
//      command list to `Renderer`.

use std::collections::HashMap;
use std::ffi::CString;
use std::num::NonZeroU32;
use std::time::Instant;

use focus::app::{App, Atlas, Drawing, IO, InputEvent, WindowId};
use focus::render::Renderer;
use glutin::config::{Config, ConfigTemplateBuilder};
use glutin::context::{ContextApi, ContextAttributesBuilder, PossiblyCurrentContext, Version};
use glutin::display::GetGlDisplay;
use glutin::prelude::*;
use glutin::surface::{Surface, SwapInterval, WindowSurface};
use glutin_winit::{DisplayBuilder, GlWindow};
use raw_window_handle::HasWindowHandle;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::platform::wayland::WindowAttributesExtWayland;
use winit::window::{Window, WindowId as WinitWindowId};

// Use a distinct title + app_id in debug builds so a niri window-rule can
// match only the dev instance (e.g. `open-focused false`).
const APP_ID: &str = if cfg!(debug_assertions) {
    "focus-debug"
} else {
    "focus"
};

struct WindowEntry {
    window: Window,
    surface: Surface<WindowSurface>,
    id: WindowId,
}

struct FocusApp {
    app: Option<App>,
    // Renderer + context + gl_config come up alongside the first window
    // — all of these need a GL display, which we don't have until glutin
    // gives us one. Hence `Option`. Subsequent windows reuse them.
    renderer: Option<Renderer>,
    context: Option<PossiblyCurrentContext>,
    gl_config: Option<Config>,
    windows: HashMap<WinitWindowId, WindowEntry>,
    by_id: HashMap<WindowId, WinitWindowId>,
    inputs: Vec<(WindowId, InputEvent)>,
    last_frame: Option<Instant>,
}

// Live `IO` impl. Constructed per pump, borrowing the parts of
// `FocusApp` that the trait methods touch.
struct Outside<'a> {
    now: Instant,
    event_loop: &'a ActiveEventLoop,
    renderer: &'a mut Option<Renderer>,
    context: &'a mut Option<PossiblyCurrentContext>,
    gl_config: &'a mut Option<Config>,
    windows: &'a mut HashMap<WinitWindowId, WindowEntry>,
    by_id: &'a mut HashMap<WindowId, WinitWindowId>,
}

impl IO for Outside<'_> {
    fn elapsed(&self) -> Instant {
        self.now
    }

    fn open_window(&mut self, window: WindowId, title: String, size: LogicalSize<u32>) {
        if self.context.is_none() {
            self.bootstrap_first_window(window, &title, size);
        } else {
            self.open_additional_window(window, &title, size);
        }
    }

    fn close_window(&mut self, window: WindowId) {
        if let Some(winit_id) = self.by_id.remove(&window) {
            self.windows.remove(&winit_id);
        }
    }

    fn set_window_title(&mut self, window: WindowId, title: String) {
        if let Some(entry) = self
            .by_id
            .get(&window)
            .and_then(|wid| self.windows.get(wid))
        {
            entry.window.set_title(&title);
        }
    }

    fn atlas_changed(&mut self, atlas: &Atlas) {
        if let Some(renderer) = self.renderer.as_ref() {
            unsafe { renderer.upload_atlas(atlas) };
        }
    }

    fn exit(&mut self) {
        self.event_loop.exit();
    }
}

impl Outside<'_> {
    // First window also brings up the GL display + context + renderer.
    // Vsync is requested here (Wait(1)) — this surface's swap throttles
    // the whole event loop. Additional windows present immediately
    // (DontWait) so they don't serialize with this one.
    fn bootstrap_first_window(&mut self, id: WindowId, title: &str, size: LogicalSize<u32>) {
        let attrs = Window::default_attributes()
            .with_title(title)
            .with_name(APP_ID, "")
            .with_inner_size(size);

        let template = ConfigTemplateBuilder::new().with_alpha_size(8);
        let display_builder = DisplayBuilder::new().with_window_attributes(Some(attrs));

        let (window, gl_config) = display_builder
            .build(self.event_loop, template, |mut configs| configs.next().unwrap())
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

        let winit_id = window.id();
        *self.context = Some(context);
        *self.renderer = Some(renderer);
        *self.gl_config = Some(gl_config);
        self.windows.insert(
            winit_id,
            WindowEntry {
                window,
                surface,
                id,
            },
        );
        self.by_id.insert(id, winit_id);
    }

    fn open_additional_window(&mut self, id: WindowId, title: &str, size: LogicalSize<u32>) {
        let attrs = Window::default_attributes()
            .with_title(title)
            .with_name(APP_ID, "")
            .with_inner_size(size);

        let window = self.event_loop.create_window(attrs).expect("create_window");
        let gl_config = self.gl_config.as_ref().unwrap();
        let gl_display = gl_config.display();

        let surface_attrs = window.build_surface_attributes(Default::default()).unwrap();
        let surface = unsafe {
            gl_display
                .create_window_surface(gl_config, &surface_attrs)
                .expect("failed to create surface")
        };

        let context = self.context.as_ref().unwrap();
        context
            .make_current(&surface)
            .expect("failed to make context current");
        // DontWait so this swap doesn't block on its own vsync — the
        // primary window's Wait(1) already throttles the loop.
        let _ = surface.set_swap_interval(context, SwapInterval::DontWait);

        let winit_id = window.id();
        self.windows.insert(
            winit_id,
            WindowEntry {
                window,
                surface,
                id,
            },
        );
        self.by_id.insert(id, winit_id);
    }
}

impl FocusApp {
    fn new() -> Self {
        FocusApp {
            app: None,
            renderer: None,
            context: None,
            gl_config: None,
            windows: HashMap::new(),
            by_id: HashMap::new(),
            inputs: Vec::new(),
            last_frame: None,
        }
    }

    fn pump(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let dt = self.last_frame.map_or(0.0, |t| (now - t).as_secs_f64());
        self.last_frame = Some(now);

        let FocusApp {
            app,
            renderer,
            context,
            gl_config,
            windows,
            by_id,
            inputs,
            ..
        } = self;
        let inputs = std::mem::take(inputs);

        let mut io = Outside {
            now,
            event_loop,
            renderer,
            context,
            gl_config,
            windows,
            by_id,
        };

        let app = match app {
            Some(a) => a,
            None => {
                let font_bytes = std::fs::read("deps/FiraCode-Regular.ttf").unwrap();
                *app = Some(App::new(font_bytes, &mut io));
                app.as_mut().unwrap()
            }
        };

        for (window, event) in inputs {
            app.input(window, event, &mut io);
        }
        app.tick(dt, &mut io);
    }

    fn draw(&mut self, winit_id: WinitWindowId) {
        let Some(entry) = self.windows.get(&winit_id) else {
            return;
        };
        let context = self.context.as_ref().unwrap();
        // Bind the shared GL context to this window's surface before we
        // render or swap. All GL state (textures, programs) is per-context
        // and shared across windows; only the default framebuffer is
        // per-surface.
        context
            .make_current(&entry.surface)
            .expect("make_current");

        let size = entry.window.inner_size();
        let fb_w = size.width as f32;
        let fb_h = size.height as f32;

        let app = self.app.as_ref().unwrap();
        let mut drawing = Drawing::new(fb_w, fb_h);
        app.draw(entry.id, &mut drawing);

        unsafe {
            self.renderer.as_mut().unwrap().render(
                drawing.commands(),
                &app.atlas,
                size.width as i32,
                size.height as i32,
            );
        }
        entry.surface.swap_buffers(context).unwrap();
    }
}

impl ApplicationHandler for FocusApp {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {
        // First-frame bootstrap (the initial `OpenWindow`) happens in
        // about_to_wait — same as every subsequent frame.
    }

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        winit_id: WinitWindowId,
        event: WindowEvent,
    ) {
        let Some(window) = self.windows.get(&winit_id).map(|e| e.id) else {
            return;
        };

        match &event {
            // We draw every window every frame in about_to_wait, so
            // OS-initiated repaints don't need extra handling.
            WindowEvent::RedrawRequested => return,
            WindowEvent::Resized(size) => {
                if let (Some(w), Some(h)) =
                    (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
                {
                    let entry = self.windows.get(&winit_id).unwrap();
                    entry.surface.resize(self.context.as_ref().unwrap(), w, h);
                }
            }
            _ => {}
        }

        self.inputs.push((window, event));
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // One frame: drain accumulated events into a single update, then
        // draw every window. `swap_buffers` blocks on vsync (set up in
        // `open_window` with `SwapInterval::Wait(1)`), which throttles
        // the loop to the display refresh rate — that's why we run
        // ControlFlow::Poll without burning CPU.
        self.pump(event_loop);
        let winit_ids: Vec<_> = self.windows.keys().copied().collect();
        for winit_id in winit_ids {
            self.draw(winit_id);
        }
    }
}

fn main() {
    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = FocusApp::new();
    event_loop.run_app(&mut app).unwrap();
}
