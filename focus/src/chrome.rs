use std::collections::HashMap;
use std::ffi::{CString, OsStr};
use std::io::{Read, Seek, SeekFrom, Write};
use std::mem::take;
use std::num::NonZeroU32;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use bstr::{BStr, BString, ByteSlice};
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
use winit::event::{StartCause, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key as WinitKey, NamedKey as WinitNamedKey};
use winit::platform::wayland::WindowAttributesExtWayland;
use winit::window::Window;

use focus_core::app::{
    App, DirEntry, INITIAL_SIZE, INITIAL_TITLE, IO, ProcessId, ProcessPoll, RepoFiles, RepoMatch,
    RepoSearch, WindowSize,
};
use focus_core::buffer;
use focus_core::drawing::Drawing;
use focus_core::input::{ButtonState, InputEvent, Key, ModifiersState, NamedKey, ScrollPhase};
use focus_core::window::WindowId;

use crate::APP_ID;
use crate::atlas::Atlas;
use crate::daemon::{self, Incoming, Request};
use crate::render::Renderer;

const FONT: &[u8] = include_bytes!("../deps/FiraCode-Regular.ttf");

const TARGET_FRAME: Duration = Duration::from_nanos(1_000_000_000 / 60);

/// Run the daemon: serve `listener` on a second thread, and turn every
/// request it reads into a window on this one.
pub fn run(listener: UnixListener) {
    let event_loop = EventLoop::<Incoming>::with_user_event().build().unwrap();
    // Nothing to draw until the first request arrives.
    event_loop.set_control_flow(ControlFlow::Wait);
    let proxy = event_loop.create_proxy();
    daemon::serve(listener, move |incoming| {
        // The loop is gone only once the daemon is exiting, and then
        // there is nothing left to deliver to.
        let _ = proxy.send_event(incoming);
    });
    let mut chrome = Chrome::Init(Init {
        resumed: false,
        pending: Vec::new(),
    });
    event_loop.run_app(&mut chrome).unwrap();
}

// Two-phase: stay in `Init` until we have both an `ActiveEventLoop` (from
// `resumed`) and something to show (a request), then transition to
// `Running` and stay there. Nothing brings up GL or opens an OS window
// before the first request, so an idle daemon holds no window at all.
enum Chrome {
    Init(Init),
    Running(Running),
}

struct Init {
    resumed: bool,
    pending: Vec<Incoming>,
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
    processes: Vec<ProcessState>,
    // One-shot processes nobody polls, reaped by reap_detached.
    detached: Vec<std::process::Child>,
    // The connection each window's request arrived on, held open until
    // the window closes so that a waiting client blocks until then.
    waiting: HashMap<WindowId, UnixStream>,
}

struct ProcessState {
    // Some until the process has been reaped (by a poll or a kill); None
    // from the start if the spawn failed.
    child: Option<std::process::Child>,
    // The reader thread appends the merged stdout/stderr stream here;
    // polls drain it. See release() for how the reader is stopped.
    output: Arc<Mutex<Vec<u8>>>,
    reader: Option<thread::JoinHandle<()>>,
    // Set when the process is reaped. The code is reported to the app once
    // the reader has drained the pipe (so the app sees the exit after the
    // last output) or, if a descendant is still holding the pipe open,
    // once EXIT_GRACE has passed since the exit.
    exit_code: Option<i32>,
    exited_at: Option<Instant>,
}

// How much unread output to buffer for a process. Past this the reader
// stops reading, so the pipe fills and the command blocks on its own
// write. That bounds memory and the work one poll can hand the app, and a
// command producing output without end throttles itself instead of
// locking up the editor.
const MAX_BUFFERED_OUTPUT: usize = 128 * 1024;

// How long the reader waits before checking again once that limit is hit.
const READER_WAIT: Duration = Duration::from_millis(2);

// How long after the shell exits to wait for its pipe to close before
// reporting the exit anyway. The pipe normally closes within microseconds
// of the exit; only a descendant that inherited it keeps it open longer.
const EXIT_GRACE: Duration = Duration::from_millis(100);

impl ProcessState {
    // Called once the process has been reaped and nothing more will be
    // read from it: stop the reader and drop the buffered output. The
    // Vec entry stays, so ProcessIds remain valid, but the retained state
    // is small.
    //
    // The reader is never joined: a descendant that inherited the pipe (a
    // daemon, a background job) can hold its write end open indefinitely,
    // and joining would hang the UI thread until it exits. Instead we drop
    // our handle on the output buffer; the reader notices it is the sole
    // owner at its next read and stops.
    fn release(&mut self) {
        self.reader = None;
        self.output = Arc::new(Mutex::new(Vec::new()));
    }
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

    fn log(&mut self, message: std::fmt::Arguments<'_>) {
        eprintln!("{message}");
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

    fn file_read_at(
        &mut self,
        path: &Path,
        offset: usize,
        limit: usize,
    ) -> std::io::Result<Vec<u8>> {
        let mut file = std::fs::File::open(path)?;
        if offset > 0 {
            file.seek(SeekFrom::Start(offset as u64))?;
        }
        let mut contents = Vec::new();
        file.take(limit as u64).read_to_end(&mut contents)?;
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

    fn canonical_path(&mut self, path: &Path) -> PathBuf {
        canonical_path(path)
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

    fn repo_search(
        &mut self,
        dir: &Path,
        pattern: &BStr,
        match_limit: usize,
        line_limit: usize,
    ) -> std::io::Result<RepoSearch> {
        repo_search(dir, pattern, match_limit, line_limit)
    }

    fn repo_root(&mut self, dir: &Path) -> PathBuf {
        git_root(dir)
    }

    fn home_dir(&mut self) -> PathBuf {
        std::env::home_dir().unwrap_or_else(|| PathBuf::from("/"))
    }

    fn process_spawn(&mut self, dir: &Path, command: &BStr, args: &[&BStr]) -> ProcessId {
        self.backend.process_spawn(dir, command, args)
    }

    fn process_poll(&mut self, id: ProcessId) -> ProcessPoll {
        self.backend.process_poll(id)
    }

    fn process_kill(&mut self, id: ProcessId) {
        self.backend.process_kill(id)
    }

    fn process_spawn_detached(&mut self, dir: &Path, command: &BStr, args: &[&BStr]) {
        self.backend.process_spawn_detached(dir, command, args)
    }
}

// Collects one RepoMatch per pattern occurrence within each matching line.
struct MatchSink<'a> {
    matcher: &'a grep_regex::RegexMatcher,
    relative_path: &'a Path,
    match_limit: usize,
    line_limit: usize,
    matches: &'a mut Vec<RepoMatch>,
}

impl grep_searcher::Sink for MatchSink<'_> {
    type Error = std::io::Error;

    fn matched(
        &mut self,
        _searcher: &grep_searcher::Searcher,
        sink_match: &grep_searcher::SinkMatch<'_>,
    ) -> Result<bool, Self::Error> {
        use grep_matcher::Matcher;

        // The searcher is built with line_number(true), so this is Some.
        let line = (sink_match.line_number().unwrap() - 1) as usize;
        let base = sink_match.absolute_byte_offset() as usize;
        let bytes = sink_match.bytes();
        let mut line_text = bytes;
        if let Some(stripped) = line_text.strip_suffix(b"\n") {
            line_text = stripped;
        }
        if let Some(stripped) = line_text.strip_suffix(b"\r") {
            line_text = stripped;
        }
        // A minified file has megabytes on one line, and every match on that
        // line would keep its own copy.
        let line_text = &line_text[..line_text.len().min(self.line_limit)];
        self.matcher
            .find_iter(bytes, |found| {
                self.matches.push(RepoMatch {
                    relative_path: self.relative_path.to_path_buf(),
                    line,
                    range: base + found.start()..base + found.end(),
                    line_text: line_text.into(),
                });
                // One more than the limit is enough to know the search was
                // truncated.
                self.matches.len() <= self.match_limit
            })
            .map_err(std::io::Error::other)?;
        Ok(self.matches.len() <= self.match_limit)
    }
}

pub fn repo_search(
    dir: &Path,
    pattern: &BStr,
    match_limit: usize,
    line_limit: usize,
) -> std::io::Result<RepoSearch> {
    let root = git_root(dir);
    let pattern = pattern.to_str().map_err(std::io::Error::other)?;
    let matcher = grep_regex::RegexMatcherBuilder::new()
        .fixed_strings(true)
        .build(pattern)
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let mut searcher = grep_searcher::SearcherBuilder::new()
        .line_number(true)
        .binary_detection(grep_searcher::BinaryDetection::quit(0))
        .build();
    let mut matches = Vec::new();
    for result in ignore::WalkBuilder::new(&root).build() {
        // Stop walking once the limit is passed, rather than searching the
        // whole repo and throwing the rest away. One match past the limit is
        // enough to know the search was truncated.
        if matches.len() > match_limit {
            break;
        }
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
        searcher.search_path(
            &matcher,
            entry.path(),
            MatchSink {
                matcher: &matcher,
                relative_path,
                match_limit,
                line_limit,
                matches: &mut matches,
            },
        )?;
    }
    let truncated = matches.len() > match_limit;
    matches.truncate(match_limit);
    Ok(RepoSearch {
        root,
        matches,
        truncated,
    })
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

impl ApplicationHandler<Incoming> for Chrome {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Chrome::Init(init) = self {
            init.resumed = true;
            self.start_if_ready(event_loop);
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, incoming: Incoming) {
        match self {
            Chrome::Init(init) => {
                // A request can arrive before winit says we are active -
                // the client queues it on the socket before spawning us -
                // so hold it until there is an event loop to open on.
                init.pending.push(incoming);
                self.start_if_ready(event_loop);
            }
            Chrome::Running(running) => running.request(event_loop, incoming),
        }
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

impl Chrome {
    // Bring up GL and the app once there is both an event loop and a
    // request to serve, then hand over every request held meanwhile.
    fn start_if_ready(&mut self, event_loop: &ActiveEventLoop) {
        let Chrome::Init(init) = self else {
            return;
        };
        if !init.resumed || init.pending.is_empty() {
            return;
        }
        let pending = take(&mut init.pending);
        let (mut backend, _initial_window_id) =
            Backend::bootstrap(event_loop, INITIAL_TITLE, INITIAL_SIZE);
        let app = {
            let mut io = IoReal {
                backend: &mut backend,
                event_loop,
            };
            App::new(&mut io)
        };
        let now = Instant::now();
        let mut running = Running {
            app,
            backend,
            first_frame: now,
            last_frame: now,
        };
        for incoming in pending {
            running.request(event_loop, incoming);
        }
        *self = Chrome::Running(running);
    }
}

impl Running {
    fn request(&mut self, event_loop: &ActiveEventLoop, incoming: Incoming) {
        let Incoming {
            request,
            connection,
        } = incoming;
        let mut io = IoReal {
            backend: &mut self.backend,
            event_loop,
        };
        let window_id = match request {
            Request::Scratch => Some(focus_core::window::open_scratch(&mut self.app, &mut io)),
            // Absolute by construction: `Request::decode` only accepts a
            // path starting with `/`.
            Request::File(path) => {
                let buffer_id = buffer::from_file(&mut self.app, &mut io, path);
                Some(focus_core::window::open_edit(
                    &mut self.app,
                    &mut io,
                    buffer_id,
                ))
            }
            Request::Launcher => Some(focus_core::window::open_launcher(&mut self.app, &mut io)),
            Request::Quit => {
                focus_core::window::quit(&mut self.app, &mut io);
                None
            }
        };
        // Hold the connection until that window closes: dropping it is
        // how a waiting client learns it is done. A quit has no window,
        // so its client is released as the daemon goes down.
        if let Some(window_id) = window_id {
            self.backend.waiting.insert(window_id, connection);
        }
    }

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

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // A daemon whose windows are all closed has nothing to tick,
        // draw or animate: idle until a request wakes it, rather than
        // spinning at the frame rate for as long as it lives.
        if self.backend.windows.is_empty() {
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        }
        event_loop.set_control_flow(ControlFlow::Poll);
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
            processes: Vec::new(),
            detached: Vec::new(),
            waiting: HashMap::new(),
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
        // Releases whoever was waiting on this window.
        self.waiting.remove(&window_id);
    }

    // Drop the one-shot processes that have finished, so they don't linger
    // as zombies. Called whenever the app touches the process API, which is
    // every frame while a command is running.
    fn reap_detached(&mut self) {
        self.detached
            .retain_mut(|child| matches!(child.try_wait(), Ok(None)));
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

    fn process_spawn(&mut self, dir: &Path, command: &BStr, args: &[&BStr]) -> ProcessId {
        self.reap_detached();
        let output = Arc::new(Mutex::new(Vec::new()));
        let state = match spawn_fish(dir, command, args) {
            Ok((child, pipe)) => {
                let reader = spawn_pipe_reader(pipe, Arc::clone(&output));
                ProcessState {
                    child: Some(child),
                    output,
                    reader: Some(reader),
                    exit_code: None,
                    exited_at: None,
                }
            }
            Err(error) => {
                output
                    .lock()
                    .unwrap()
                    .extend_from_slice(format!("Failed to spawn fish: {}", error).as_bytes());
                ProcessState {
                    child: None,
                    output,
                    reader: None,
                    exit_code: Some(-1),
                    exited_at: Some(Instant::now()),
                }
            }
        };
        self.processes.push(state);
        ProcessId(self.processes.len() - 1)
    }

    fn process_poll(&mut self, id: ProcessId) -> ProcessPoll {
        self.reap_detached();
        let process = &mut self.processes[id.0];
        // Reap the shell as soon as it exits, whether or not its pipe is
        // still held open by something it left behind.
        if let Some(child) = &mut process.child
            && let Ok(Some(status)) = child.try_wait()
        {
            process.child = None;
            process.exit_code = Some(status.code().unwrap_or(-1));
            process.exited_at = Some(Instant::now());
        }
        let new_output = take(&mut *process.output.lock().unwrap());
        let reader_finished = process
            .reader
            .as_ref()
            .is_none_or(|reader| reader.is_finished());
        let exit_code = match (process.exit_code, process.exited_at) {
            (Some(code), Some(exited_at))
                if reader_finished || exited_at.elapsed() >= EXIT_GRACE =>
            {
                Some(code)
            }
            _ => None,
        };
        // Once the process has exited and its pipe has closed, nothing more
        // can arrive; free everything but the exit code. While a descendant
        // holds the pipe, keep draining so the app can show the tail.
        if exit_code.is_some() && reader_finished {
            process.release();
        }
        ProcessPoll {
            new_output,
            exit_code,
        }
    }

    fn process_kill(&mut self, id: ProcessId) {
        let process = &mut self.processes[id.0];
        // Only signal the group while the child is unreaped: after it has
        // been reaped the pid (and so the group id) may have been reused by
        // an unrelated process.
        if let Some(mut child) = process.child.take() {
            // Negative pid signals the process group (see spawn_fish).
            unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) };
            let _ = child.kill();
            // Reap, so the child doesn't linger as a zombie. This waits
            // only for the shell itself, which was just killed, not for
            // anything holding its pipe.
            process.exit_code = Some(match child.wait() {
                Ok(status) => status.code().unwrap_or(-1),
                Err(_) => -1,
            });
            process.exited_at = Some(Instant::now());
        }
        // Nothing polls a killed process again; free the rest of its state.
        process.release();
    }

    fn process_spawn_detached(&mut self, dir: &Path, command: &BStr, args: &[&BStr]) {
        self.reap_detached();
        let spawned = fish_command(dir, command, args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        match spawned {
            Ok(child) => self.detached.push(child),
            Err(error) => eprintln!("failed to spawn fish: {}", error),
        }
    }
}

// Build the fish invocation shared by both spawn paths, leaving only the
// output redirection to the caller. `--` stops fish parsing an arg that
// starts with `-` as its own option, so any text can be passed through
// verbatim; it is harmless when there are no args. process_group(0) puts
// the child in its own process group, so process_kill can signal the whole
// group.
fn fish_command(dir: &Path, command: &BStr, args: &[&BStr]) -> std::process::Command {
    let mut fish = std::process::Command::new("fish");
    fish.arg("--command")
        .arg(OsStr::from_bytes(command))
        .arg("--")
        .args(args.iter().map(|arg| OsStr::from_bytes(arg)))
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .process_group(0);
    fish
}

// Run `command` under fish with stdout and stderr sharing a single pipe, so
// the two streams are merged by the kernel in write order rather than by
// racing reader threads.
fn spawn_fish(
    dir: &Path,
    command: &BStr,
    args: &[&BStr],
) -> std::io::Result<(std::process::Child, std::io::PipeReader)> {
    let (reader, writer) = std::io::pipe()?;
    let writer_for_stderr = writer.try_clone()?;
    // The Command (and with it our copies of the write end) is dropped at
    // the end of this statement, so the reader sees EOF once every process
    // that inherited the pipe has closed it.
    let child = fish_command(dir, command, args)
        .stdout(std::process::Stdio::from(writer))
        .stderr(std::process::Stdio::from(writer_for_stderr))
        .spawn()?;
    Ok((child, reader))
}

fn spawn_pipe_reader(
    mut pipe: impl Read + Send + 'static,
    output: Arc<Mutex<Vec<u8>>>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            // The backend released this process (see
            // ProcessState::release); nobody will read what we append, so
            // stop rather than buffer it forever.
            if Arc::strong_count(&output) == 1 {
                break;
            }
            // Wait for the app to catch up rather than buffering without
            // limit. The command blocks writing to a full pipe meanwhile.
            if output.lock().unwrap().len() >= MAX_BUFFERED_OUTPUT {
                thread::sleep(READER_WAIT);
                continue;
            }
            match pipe.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => output.lock().unwrap().extend_from_slice(&buf[..n]),
            }
        }
    })
}

/// See `IO::canonical_path`. A free function so it can be tested without
/// bringing up a window and a GL context.
pub fn canonical_path(path: &Path) -> PathBuf {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return canonical;
    }
    // The file may not exist yet - `focus new-notes.txt` is how you make
    // one - so fall back to canonicalizing the dir it will live in.
    // Resolving the dir is what matters: that is where the `..` and the
    // symlinks are.
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => match std::fs::canonicalize(parent) {
            Ok(parent) => parent.join(name),
            Err(_) => path.to_path_buf(),
        },
        _ => path.to_path_buf(),
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
        // A mouse wheel reports notches, a touchpad reports the pixels
        // the fingers moved. They scroll differently - the touchpad
        // gesture drags the content around and has momentum - so they
        // stay apart all the way into the editor.
        WindowEvent::MouseWheel { delta, phase, .. } => match delta {
            winit::event::MouseScrollDelta::LineDelta(_, y) => {
                Some(InputEvent::MouseWheel { y_lines: *y })
            }
            winit::event::MouseScrollDelta::PixelDelta(p) => Some(InputEvent::TouchpadScroll {
                y_pixels: p.y as f32,
                phase: match phase {
                    TouchPhase::Started => ScrollPhase::Started,
                    TouchPhase::Moved => ScrollPhase::Moved,
                    // Cancelled means the gesture was taken over by
                    // something else, so it is over either way.
                    TouchPhase::Ended | TouchPhase::Cancelled => ScrollPhase::Ended,
                },
            }),
        },
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
