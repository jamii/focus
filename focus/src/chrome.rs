use std::collections::HashMap;
use std::ffi::CString;
use std::io::{Read, Write};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use bstr::BString;
use fontdue::{Font, FontSettings};
use glutin::config::{Config, ConfigTemplateBuilder};
use glutin::context::{ContextApi, ContextAttributesBuilder, PossiblyCurrentContext, Version};
use glutin::display::GetGlDisplay;
use glutin::prelude::*;
use glutin::surface::{Surface, SwapInterval, WindowSurface};
use glutin_winit::{DisplayBuilder, GlWindow};
use raw_window_handle::HasWindowHandle;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key as WinitKey, NamedKey as WinitNamedKey};
use winit::platform::wayland::WindowAttributesExtWayland;
use winit::window::Window;

use focus_core::app::{App, DirEntry, INITIAL_SIZE, INITIAL_TITLE, IO, RepoFiles, WindowSize};
use focus_core::buffer;
use focus_core::drawing::Drawing;
use focus_core::input::{ButtonState, InputEvent, Key, ModifiersState, NamedKey};
use focus_core::window::WindowId;

use crate::atlas::Atlas;
use crate::render::Renderer;

const FONT: &[u8] = include_bytes!("../deps/FiraCode-Regular.ttf");

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
    gl_config: Config,
    context: PossiblyCurrentContext,
    renderer: Renderer,
    font: Font,
    clipboard: arboard::Clipboard,
    last_mouse_position: PhysicalPosition<f64>,
}

// Held only across an app callback; carries the live `ActiveEventLoop`
// (needed for `create_window`) plus the backend it mutates.
struct IoReal<'a> {
    backend: &'a mut Backend,
    event_loop: &'a ActiveEventLoop,
}

impl IO for IoReal<'_> {
    fn open_window(&mut self, window_id: WindowId, title: String, size: WindowSize) {
        self.backend
            .open_window(self.event_loop, window_id, &title, size);
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

    fn rebuild_atlas(&mut self, font_size: f32) -> [u32; 2] {
        let atlas = Atlas::build(&self.backend.font, font_size);
        let cell_size = atlas.cell_size;
        unsafe { self.backend.renderer.upload_atlas(atlas) };
        cell_size
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

    fn file_read_prefix(&mut self, path: &Path, limit: usize) -> std::io::Result<Vec<u8>> {
        let mut contents = Vec::new();
        std::fs::File::open(path)?
            .take(limit as u64)
            .read_to_end(&mut contents)?;
        Ok(contents)
    }

    fn file_create(&mut self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .open(path)?;
        Ok(())
    }

    fn current_dir(&mut self) -> PathBuf {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"))
    }

    fn dir_list(&mut self, path: &Path) -> std::io::Result<Vec<DirEntry>> {
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            entries.push(DirEntry {
                name: entry.file_name(),
                // Follows symlinks, so a link to a dir counts as a dir.
                is_dir: std::fs::metadata(entry.path()).is_ok_and(|m| m.is_dir()),
            });
        }
        Ok(entries)
    }

    fn repo_files(&mut self, dir: &Path) -> std::io::Result<RepoFiles> {
        let root = git_root(dir);
        let mut relative_paths = Vec::new();
        for result in ignore::WalkBuilder::new(&root).build() {
            let entry = result.map_err(|error| std::io::Error::other(error.to_string()))?;
            let Some(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_file() {
                continue;
            }
            let Ok(relative_path) = entry.path().strip_prefix(&root) else {
                continue;
            };
            if relative_path.as_os_str().is_empty() {
                continue;
            }
            relative_paths.push(relative_path.to_path_buf());
        }
        Ok(RepoFiles {
            root,
            relative_paths,
        })
    }
}

fn git_root(dir: &Path) -> PathBuf {
    let mut current = Some(dir);
    while let Some(path) = current {
        if path.join(".git").exists() {
            return path.to_path_buf();
        }
        current = path.parent();
    }
    dir.to_path_buf()
}

impl ApplicationHandler for Chrome {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let initial_path = match self {
            Chrome::Init { initial_path } => initial_path.take(),
            Chrome::Running(_) => return,
        };
        let (mut backend, _initial_window_id) =
            Backend::bootstrap(event_loop, INITIAL_TITLE, INITIAL_SIZE);
        let mut app = {
            let mut io = IoReal {
                backend: &mut backend,
                event_loop,
            };
            App::new(&mut io)
        };
        let mut io = IoReal {
            backend: &mut backend,
            event_loop,
        };
        match initial_path {
            Some(path) => {
                let buffer_id = buffer::from_file(&mut app, path);
                focus_core::window::open_edit(&mut app, &mut io, buffer_id);
            }
            None => {
                focus_core::window::open_scratch(&mut app, &mut io);
            }
        }
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
        self.last_frame = Instant::now();
        let mut io = IoReal {
            backend: &mut self.backend,
            event_loop,
        };
        self.app.tick(&mut io, self.last_frame - self.first_frame);
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
        if let WindowEvent::Resized(size) = &event {
            self.backend.resize_surface(window_id, *size);
        }
        if let WindowEvent::CursorMoved { position, .. } = &event {
            self.backend.last_mouse_position = *position;
        }
        if !self.backend.windows.contains_key(&window_id) {
            return;
        }
        let Some(translated) = translate_event(&event, self.backend.last_mouse_position) else {
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
        let font = Font::from_bytes(FONT, FontSettings::default()).unwrap();

        let clipboard = arboard::Clipboard::new()
            .expect("arboard::Clipboard::new() — is a wayland or x11 session running?");
        let mut backend = Backend {
            windows: HashMap::new(),
            winit_to_window: HashMap::new(),
            gl_config,
            context,
            renderer,
            font,
            clipboard,
            last_mouse_position: PhysicalPosition { x: 0.0, y: 0.0 },
        };
        let id = WindowId(0);
        backend.register_window(id, window, surface);
        (backend, id)
    }

    fn open_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        title: &str,
        size: WindowSize,
    ) {
        if let Some(state) = self.windows.get(&window_id) {
            state.window.set_title(title);
            return;
        }

        let window = event_loop
            .create_window(window_attrs(title, size))
            .expect("create_window");
        let surface = create_surface(&self.gl_config, &window);
        self.context.make_current(&surface).expect("make_current");
        let _ = surface.set_swap_interval(&self.context, SwapInterval::DontWait);
        self.register_window(window_id, window, surface);
    }

    fn register_window(&mut self, id: WindowId, window: Window, surface: Surface<WindowSurface>) {
        let winit_id = window.id();
        assert!(
            self.windows
                .insert(id, WindowState { window, surface })
                .is_none(),
            "duplicate window id {:?}",
            id,
        );
        self.winit_to_window.insert(winit_id, id);
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

fn translate_event(
    event: &WindowEvent,
    last_mouse_position: PhysicalPosition<f64>,
) -> Option<InputEvent<'_>> {
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
        WindowEvent::MouseInput {
            state,
            button: winit::event::MouseButton::Left,
            ..
        } => Some(InputEvent::MouseButton {
            state: translate_state(*state),
            position: [last_mouse_position.x as f32, last_mouse_position.y as f32],
        }),
        WindowEvent::CursorMoved { position, .. } => Some(InputEvent::MouseMoved {
            position: [position.x as f32, position.y as f32],
        }),
        _ => None,
    }
}

fn translate_state(state: winit::event::ElementState) -> ButtonState {
    match state {
        winit::event::ElementState::Pressed => ButtonState::Pressed,
        winit::event::ElementState::Released => ButtonState::Released,
    }
}

fn translate_key(key: &WinitKey) -> Option<Key<'_>> {
    Some(match key {
        WinitKey::Character(s) => Key::Character(s.as_str()),
        WinitKey::Named(named) => Key::Named(translate_named(*named)?),
        _ => return None,
    })
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
