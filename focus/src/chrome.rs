use std::collections::HashMap;
use std::ffi::CString;
use std::io::Write;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use bstr::BString;
use glutin::config::{Config, ConfigTemplateBuilder};
use glutin::context::{ContextApi, ContextAttributesBuilder, PossiblyCurrentContext, Version};
use glutin::display::GetGlDisplay;
use glutin::prelude::*;
use glutin::surface::{Surface, SwapInterval, WindowSurface};
use glutin_winit::{DisplayBuilder, GlWindow};
use raw_window_handle::HasWindowHandle;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key as WinitKey, NamedKey as WinitNamedKey};
use winit::platform::wayland::WindowAttributesExtWayland;
use winit::window::Window;

use focus_core::app::{App, INITIAL_SIZE, INITIAL_TITLE, IO, InputEvent, WindowId, WindowSize};
use focus_core::drawing::Drawing;
use focus_core::input::{ElementState, Key, ModifiersState, NamedKey};

use crate::render::Renderer;

// Distinct title + app_id in debug builds so a niri window-rule can
// match only the dev instance (e.g. `open-focused false`).
const APP_ID: &str = if cfg!(debug_assertions) {
    "focus-debug"
} else {
    "focus"
};

const TARGET_FRAME: Duration = Duration::from_nanos(1_000_000_000 / 60);

pub fn run(initial_path: Option<PathBuf>) {
    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut chrome = Chrome::Init { initial_path };
    event_loop.run_app(&mut chrome).unwrap();
}

// Two-phase: stay in `Init` until the first `resumed` gives us an
// `ActiveEventLoop`, then transition to `Running` and stay there.
enum Chrome {
    Init { initial_path: Option<PathBuf> },
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
    winit_to_window: HashMap<winit::window::WindowId, WindowId>,
    next_window_id: WindowId,
    gl_config: Config,
    context: PossiblyCurrentContext,
    renderer: Renderer,
    last_mouse_pos: [f32; 2],
    clipboard: arboard::Clipboard,
}

// Held only across an app callback; carries the live `ActiveEventLoop`
// (needed for `create_window`) plus the backend it mutates.
struct IoReal<'a> {
    backend: &'a mut Backend,
    event_loop: &'a ActiveEventLoop,
}

impl IO for IoReal<'_> {
    fn open_window(&mut self, title: String, size: WindowSize) -> WindowId {
        self.backend.open_window(self.event_loop, &title, size)
    }

    fn close_window(&mut self, window_id: WindowId) {
        self.backend.close_window(window_id);
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

    fn reload_atlas(&mut self, pixels: &[u8], size: [u32; 2]) {
        unsafe { self.backend.renderer.upload_atlas(pixels, size) };
    }

    fn get_clipboard_text(&mut self) -> Option<BString> {
        self.backend.clipboard.get_text().ok().map(|s| s.into())
    }

    fn set_clipboard_text(&mut self, text: BString) {
        // TODO Do the OS apis really not allow non-utf8 copy/paste?
        let _ = self.backend.clipboard.set_text(text.to_string());
    }

    fn exit(&mut self) {
        self.event_loop.exit();
    }

    fn file_mtime(&mut self, path: &Path) -> std::io::Result<SystemTime> {
        std::fs::metadata(path)?.modified()
    }

    fn file_read(&mut self, path: &Path) -> std::io::Result<Vec<u8>> {
        std::fs::read(path)
    }

    fn file_write(
        &mut self,
        path: &Path,
        contents: &[u8],
        create: bool,
    ) -> std::io::Result<SystemTime> {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).truncate(true).create(create);
        let mut f = opts.open(path)?;
        f.write_all(contents)?;
        f.metadata()?.modified()
    }
}

impl ApplicationHandler for Chrome {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let initial_path = match self {
            Chrome::Init { initial_path } => initial_path.take(),
            Chrome::Running(_) => return,
        };
        let (mut backend, initial_window_id) =
            Backend::bootstrap(event_loop, INITIAL_TITLE, INITIAL_SIZE);
        let app = {
            let mut io = IoReal {
                backend: &mut backend,
                event_loop,
            };
            App::new(initial_window_id, &mut io, initial_path)
        };
        let now = Instant::now();
        *self = Chrome::Running(Running {
            app,
            backend,
            first_frame: now,
            last_frame: now,
        });
    }

    fn new_events(&mut self, event_loop: &ActiveEventLoop, _cause: StartCause) {
        let Chrome::Running(running) = self else {
            return;
        };
        running.new_events(event_loop);
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
        let Some(window_id) = running.backend.winit_to_window.get(&window_id).copied() else {
            return;
        };
        running.window_event(event_loop, window_id, event);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Chrome::Running(running) = self else {
            return;
        };
        running.about_to_wait(event_loop);
    }
}

impl Running {
    fn new_events(&mut self, event_loop: &ActiveEventLoop) {
        let frame_start = Instant::now();
        self.last_frame = frame_start;
        self.app.frame_start = self.last_frame - self.first_frame;
        self.app.mouse_position = self.backend.last_mouse_pos;

        let mut io = IoReal {
            backend: &mut self.backend,
            event_loop,
        };
        self.app.tick(&mut io);
    }

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
        if let WindowEvent::CursorMoved { position, .. } = &event {
            self.backend.last_mouse_pos = [position.x as f32, position.y as f32];
        }
        if let WindowEvent::Resized(size) = &event {
            self.backend.resize_surface(window_id, *size);
        }
        if !self.backend.windows.contains_key(&window_id) {
            return;
        }
        let Some(translated) = translate_event(&event, self.backend.last_mouse_pos) else {
            return;
        };
        let mut io = IoReal {
            backend: &mut self.backend,
            event_loop,
        };
        self.app.input(&mut io, window_id, translated);
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        // Sleep the remainder of the frame budget.
        // winit's `Poll` mode would otherwise spin.
        let elapsed = self.last_frame.elapsed();
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
        unsafe { self.backend.renderer.render(&drawing.commands, fb_w, fb_h) };
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
    fn bootstrap(event_loop: &ActiveEventLoop, title: &str, size: WindowSize) -> (Self, WindowId) {
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

        let clipboard = arboard::Clipboard::new()
            .expect("arboard::Clipboard::new() — is a wayland or x11 session running?");
        let mut backend = Backend {
            windows: HashMap::new(),
            winit_to_window: HashMap::new(),
            next_window_id: WindowId(0),
            gl_config,
            context,
            renderer,
            last_mouse_pos: [0.0, 0.0],
            clipboard,
        };
        let id = backend.register_window(window, surface);
        (backend, id)
    }

    fn open_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        title: &str,
        size: WindowSize,
    ) -> WindowId {
        let window = event_loop
            .create_window(window_attrs(title, size))
            .expect("create_window");
        let surface = create_surface(&self.gl_config, &window);
        self.context.make_current(&surface).expect("make_current");
        let _ = surface.set_swap_interval(&self.context, SwapInterval::DontWait);
        self.register_window(window, surface)
    }

    // Mint a fresh core `WindowId` for a winit window and record both
    // directions of the id mapping.
    fn register_window(&mut self, window: Window, surface: Surface<WindowSurface>) -> WindowId {
        let winit_id = window.id();
        let id = self.next_window_id;
        self.next_window_id.0 += 1;
        self.windows.insert(id, WindowState { window, surface });
        self.winit_to_window.insert(winit_id, id);
        id
    }

    fn close_window(&mut self, window_id: WindowId) {
        if let Some(state) = self.windows.remove(&window_id) {
            self.winit_to_window.remove(&state.window.id());
        }
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

fn window_attrs(title: &str, size: WindowSize) -> winit::window::WindowAttributes {
    Window::default_attributes()
        .with_title(title)
        .with_name(APP_ID, "")
        .with_inner_size(LogicalSize::new(size.width, size.height))
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

// --- winit -> focus_core input translation ---

// Map a winit window event to the core's `InputEvent`, or `None` for
// events the core doesn't consume. Borrows `event` so the resulting
// `Key` can point straight at winit's key string.
fn translate_event(event: &WindowEvent, last_mouse_pos: [f32; 2]) -> Option<InputEvent<'_>> {
    match event {
        WindowEvent::CloseRequested => Some(InputEvent::CloseRequested),
        WindowEvent::Focused(focused) => Some(InputEvent::FocusChanged { focused: *focused }),
        WindowEvent::ModifiersChanged(m) => {
            Some(InputEvent::ModifiersChanged(translate_modifiers(m.state())))
        }
        WindowEvent::KeyboardInput { event, .. } => {
            translate_key(&event.logical_key).map(|logical_key| InputEvent::Key {
                state: translate_state(event.state),
                logical_key,
            })
        }
        WindowEvent::MouseWheel { delta, .. } => {
            let y_offset = match delta {
                winit::event::MouseScrollDelta::LineDelta(_, y) => *y,
                winit::event::MouseScrollDelta::PixelDelta(p) => (p.y as f32) / 32.0,
            };
            Some(InputEvent::MouseWheel { y_offset })
        }
        WindowEvent::MouseInput { state, button, .. } => {
            if *button == winit::event::MouseButton::Left {
                Some(InputEvent::MouseButton {
                    state: translate_state(*state),
                    position: last_mouse_pos,
                })
            } else {
                None
            }
        }
        _ => None,
    }
}

fn translate_state(state: winit::event::ElementState) -> ElementState {
    match state {
        winit::event::ElementState::Pressed => ElementState::Pressed,
        winit::event::ElementState::Released => ElementState::Released,
    }
}

// Text keys carry their string; named keys map to the subset the editor
// understands. Anything else (dead keys, unidentified, unhandled named
// keys) yields None and is dropped.
fn translate_key(key: &WinitKey) -> Option<Key<'_>> {
    match key {
        WinitKey::Character(s) => Some(Key::Character(s.as_str())),
        WinitKey::Named(named) => Some(Key::Named(translate_named(*named)?)),
        _ => None,
    }
}

fn translate_named(named: WinitNamedKey) -> Option<NamedKey> {
    Some(match named {
        WinitNamedKey::Enter => NamedKey::Enter,
        WinitNamedKey::Space => NamedKey::Space,
        WinitNamedKey::Backspace => NamedKey::Backspace,
        WinitNamedKey::Delete => NamedKey::Delete,
        WinitNamedKey::ArrowLeft => NamedKey::ArrowLeft,
        WinitNamedKey::ArrowRight => NamedKey::ArrowRight,
        WinitNamedKey::ArrowUp => NamedKey::ArrowUp,
        WinitNamedKey::ArrowDown => NamedKey::ArrowDown,
        _ => return None,
    })
}

fn translate_modifiers(state: winit::keyboard::ModifiersState) -> ModifiersState {
    ModifiersState {
        control: state.control_key(),
        alt: state.alt_key(),
        shift: state.shift_key(),
        super_: state.super_key(),
    }
}
