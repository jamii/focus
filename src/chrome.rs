// Plumbing between winit/glutin and `app::App`.
//
// Lifecycle:
//   * On `resumed` we bootstrap one window with its GL display, context,
//     and renderer, then build the `App` against that window. After this
//     point everything we care about (context, renderer, app) is
//     unconditionally present — no `Option` dance.
//   * Each loop iteration is paced by a fixed-rate timing loop: tick,
//     then `thread::sleep` for whatever's left of the frame budget.
//     winit runs in `ControlFlow::Poll`, so the loop is ours to throttle.
//   * Input events go straight to `app.input`. The app may call
//     `io.request_redraw`, which schedules a `WindowEvent::RedrawRequested`
//     for the same iteration; we draw on that. So input → pixels stays
//     on one loop pass with no queued-for-next-frame lag.
//
// No vsync: with many windows, per-surface vsync serializes swaps. The
// timing loop's sleep already caps us at ~60 Hz, which on most displays
// is also the refresh rate.

use std::collections::HashMap;
use std::ffi::CString;
use std::num::NonZeroU32;
use std::thread;
use std::time::{Duration, Instant};

use glutin::config::{Config, ConfigTemplateBuilder};
use glutin::context::{ContextApi, ContextAttributesBuilder, PossiblyCurrentContext, Version};
use glutin::display::GetGlDisplay;
use glutin::prelude::*;
use glutin::surface::{Surface, SwapInterval, WindowSurface};
use glutin_winit::{DisplayBuilder, GlWindow};
use raw_window_handle::HasWindowHandle;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::platform::wayland::WindowAttributesExtWayland;
use winit::window::Window;

use crate::app::{App, INITIAL_SIZE, INITIAL_TITLE, IO, WindowId};
use crate::render::Renderer;
use crate::text::{Atlas, Drawing};

// Distinct title + app_id in debug builds so a niri window-rule can
// match only the dev instance (e.g. `open-focused false`).
const APP_ID: &str = if cfg!(debug_assertions) {
    "focus-debug"
} else {
    "focus"
};

const TARGET_FRAME: Duration = Duration::from_nanos(1_000_000_000 / 60);

pub fn run() {
    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut chrome = Chrome::Init;
    event_loop.run_app(&mut chrome).unwrap();
}

// Two-phase: stay in `Init` until the first `resumed` gives us an
// `ActiveEventLoop`, then transition to `Running` and stay there.
enum Chrome {
    Init,
    Running(Running),
}

struct Running {
    app: App,
    backend: Backend,
    first_frame: Instant,
    last_frame: Instant,
}

struct WindowState {
    window: Window,
    surface: Surface<WindowSurface>,
}

struct Backend {
    windows: HashMap<WindowId, WindowState>,
    gl_config: Config,
    context: PossiblyCurrentContext,
    renderer: Renderer,
}

// Held only across an app callback; carries the live `ActiveEventLoop`
// (needed for `create_window`) plus the backend it mutates.
struct IoReal<'a> {
    frame_start: Duration,
    backend: &'a mut Backend,
    event_loop: &'a ActiveEventLoop,
}

impl IO for IoReal<'_> {
    fn frame_start(&self) -> Duration {
        self.frame_start
    }

    fn open_window(&mut self, title: String, size: LogicalSize<u32>) -> WindowId {
        self.backend.open_window(self.event_loop, &title, size)
    }

    fn close_window(&mut self, window_id: WindowId) {
        self.backend.windows.remove(&window_id);
    }

    fn set_window_title(&mut self, window_id: WindowId, title: String) {
        if let Some(w) = self.backend.windows.get(&window_id) {
            w.window.set_title(&title);
        }
    }

    fn request_redraw(&mut self, window_id: WindowId) {
        if let Some(w) = self.backend.windows.get(&window_id) {
            w.window.request_redraw();
        }
    }

    fn reload_atlas(&mut self, atlas: &Atlas) {
        unsafe { self.backend.renderer.upload_atlas(atlas) };
    }

    fn exit(&mut self) {
        self.event_loop.exit();
    }
}

impl ApplicationHandler for Chrome {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if matches!(self, Chrome::Running(_)) {
            return;
        }
        let (mut backend, initial_window_id) =
            Backend::bootstrap(event_loop, INITIAL_TITLE, INITIAL_SIZE);
        let app = {
            let mut io = IoReal {
                frame_start: Duration::ZERO,
                backend: &mut backend,
                event_loop,
            };
            App::new(initial_window_id, &mut io)
        };
        let now = Instant::now();
        *self = Chrome::Running(Running {
            app,
            backend,
            first_frame: now,
            last_frame: now,
        });
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let Chrome::Running(running) = self else {
            return;
        };
        running.window_event(event_loop, WindowId(window_id), event);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Chrome::Running(running) = self else {
            return;
        };
        running.about_to_wait(event_loop);
    }
}

impl Running {
    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        // Redraws aren't input — handle here, don't forward to app.
        if matches!(event, WindowEvent::RedrawRequested) {
            self.draw(window_id);
            return;
        }
        if let WindowEvent::Resized(size) = &event {
            self.backend.resize_surface(window_id, *size);
        }
        if !self.backend.windows.contains_key(&window_id) {
            return;
        }
        let mut io = IoReal {
            frame_start: self.last_frame - self.first_frame,
            backend: &mut self.backend,
            event_loop,
        };
        self.app.input(&mut io, window_id, event);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let frame_start = Instant::now();
        self.last_frame = frame_start;

        let mut io = IoReal {
            frame_start: self.last_frame - self.first_frame,
            backend: &mut self.backend,
            event_loop,
        };
        self.app.tick(&mut io);

        // Sleep the remainder of the frame budget. winit's `Poll` mode
        // would otherwise spin; the sleep is our throttle.
        let elapsed = frame_start.elapsed();
        if elapsed < TARGET_FRAME {
            thread::sleep(TARGET_FRAME - elapsed);
        }
    }

    fn draw(&mut self, window_id: WindowId) {
        let Some(window_state) = self.backend.windows.get(&window_id) else {
            return;
        };
        let context = &self.backend.context;
        // GL objects (program, VAO, atlas texture) are shared across
        // windows via the shared context; only the default framebuffer
        // is per-surface, so we rebind to this surface and render.
        context
            .make_current(&window_state.surface)
            .expect("make_current");
        let size = window_state.window.inner_size();
        let fb_w = size.width as i32;
        let fb_h = size.height as i32;
        let mut drawing = Drawing::new([fb_w as f32, fb_h as f32]);
        self.app.draw(window_id, &mut drawing);
        unsafe { self.backend.renderer.render(drawing.commands(), fb_w, fb_h) };
        window_state
            .surface
            .swap_buffers(context)
            .expect("swap_buffers");
    }
}

impl Backend {
    // Brings up the GL display, picks a config, creates the context,
    // loads `gl::*` against the now-current context, and constructs the
    // renderer. Returns the freshly-opened window's id alongside the
    // populated `Backend`.
    fn bootstrap(
        event_loop: &ActiveEventLoop,
        title: &str,
        size: LogicalSize<u32>,
    ) -> (Self, WindowId) {
        let attrs = window_attrs(title, size);
        let template = ConfigTemplateBuilder::new().with_alpha_size(8);
        let (window, gl_config) = DisplayBuilder::new()
            .with_window_attributes(Some(attrs))
            .build(event_loop, template, |mut iter| iter.next().unwrap())
            .expect("build display");
        let window = window.expect("window");

        let gl_display = gl_config.display();
        let raw_window_handle = window.window_handle().unwrap().as_raw();
        let context_attrs = ContextAttributesBuilder::new()
            .with_context_api(ContextApi::OpenGl(Some(Version::new(3, 3))))
            .build(Some(raw_window_handle));
        let not_current = unsafe {
            gl_display
                .create_context(&gl_config, &context_attrs)
                .expect("create_context")
        };
        let surface = create_surface(&gl_config, &window);
        let context = not_current.make_current(&surface).expect("make_current");
        let _ = surface.set_swap_interval(&context, SwapInterval::DontWait);

        // Function pointers can only be loaded once a context is current.
        gl::load_with(|s| {
            let cs = CString::new(s).unwrap();
            gl_display.get_proc_address(&cs) as *const _
        });
        let renderer = unsafe { Renderer::new() };

        let id = WindowId(window.id());
        // Initial paint — drives the first `RedrawRequested` so the
        // window doesn't stay blank until something dirties it.
        window.request_redraw();
        let mut windows = HashMap::new();
        windows.insert(id, WindowState { window, surface });
        let backend = Backend {
            windows,
            gl_config,
            context,
            renderer,
        };
        (backend, id)
    }

    fn open_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        title: &str,
        size: LogicalSize<u32>,
    ) -> WindowId {
        let window = event_loop
            .create_window(window_attrs(title, size))
            .expect("create_window");
        let surface = create_surface(&self.gl_config, &window);
        self.context.make_current(&surface).expect("make_current");
        let _ = surface.set_swap_interval(&self.context, SwapInterval::DontWait);
        let id = WindowId(window.id());
        window.request_redraw();
        self.windows.insert(id, WindowState { window, surface });
        id
    }

    fn resize_surface(&mut self, window_id: WindowId, size: PhysicalSize<u32>) {
        let Some(state) = self.windows.get(&window_id) else {
            return;
        };
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
            return;
        };
        state.surface.resize(&self.context, w, h);
    }
}

fn window_attrs(title: &str, size: LogicalSize<u32>) -> winit::window::WindowAttributes {
    Window::default_attributes()
        .with_title(title)
        .with_name(APP_ID, "")
        .with_inner_size(size)
}

fn create_surface(gl_config: &Config, window: &Window) -> Surface<WindowSurface> {
    let surface_attrs = window
        .build_surface_attributes(Default::default())
        .expect("surface_attrs");
    unsafe {
        gl_config
            .display()
            .create_window_surface(gl_config, &surface_attrs)
            .expect("create_window_surface")
    }
}
