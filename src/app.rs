use std::collections::HashSet;
use std::time::Duration;

use winit::dpi::LogicalSize;
use winit::event::ElementState;
use winit::keyboard::{Key, NamedKey};
use winit::window::WindowId;

pub use crate::text::{Atlas, Drawing, Rect};
use crate::text::{Font, FontSettings};

pub struct App {
    windows: HashSet<WindowId>,
    font: Font,
    px_size: f32,
    atlas: Atlas,
}

pub type InputEvent = winit::event::WindowEvent;

// External effects app may need. Mocked for testing/fuzzing etc.
pub trait IO {
    fn elapsed(&self) -> Duration;
    fn open_window(&mut self, title: String, size: LogicalSize<u32>) -> WindowId;
    fn close_window(&mut self, window: WindowId);
    fn set_window_title(&mut self, window: WindowId, title: String);
    fn request_redraw(&mut self, window: WindowId);
    fn reload_atlas(&mut self, atlas: &Atlas);
    fn exit(&mut self);
}

const FONT: &[u8] = include_bytes!("../deps/FiraCode-Regular.ttf");
const INITIAL_PX: f32 = 32.0;
const MIN_PX: f32 = 4.0;
pub const INITIAL_TITLE: &str = "focus";
pub const INITIAL_SIZE: LogicalSize<u32> = LogicalSize {
    width: 800,
    height: 600,
};

// Placeholder scene — each window draws its own id so the multi-window
// path is visibly distinct per window.
const TEXT_X: f32 = 20.0;
const TEXT_Y: f32 = 20.0;
const TEXT_COLOR: [u8; 4] = [30, 30, 40, 255];
const HIGHLIGHT_COLOR: [u8; 4] = [255, 240, 170, 255];

impl App {
    pub fn new(initial_window: WindowId, io: &mut dyn IO) -> App {
        let font = Font::from_bytes(FONT, FontSettings::default()).unwrap();
        let atlas = Atlas::build(&font, INITIAL_PX);
        let mut windows = HashSet::new();
        windows.insert(initial_window);
        io.reload_atlas(&atlas);
        App {
            windows,
            font,
            px_size: INITIAL_PX,
            atlas,
        }
    }

    pub fn input(&mut self, window: WindowId, event: InputEvent, io: &mut dyn IO) {
        match event {
            InputEvent::CloseRequested => {
                self.windows.remove(&window);
                io.close_window(window);
                if self.windows.is_empty() {
                    io.exit();
                }
            }
            InputEvent::KeyboardInput {
                event: key_event, ..
            } if key_event.state == ElementState::Pressed => match key_event.logical_key.as_ref() {
                Key::Named(NamedKey::Escape) => io.exit(),
                Key::Character("+") => {
                    self.px_size += 1.0;
                    self.rebuild_atlas(io);
                }
                Key::Character("-") => {
                    self.px_size = (self.px_size - 1.0).max(MIN_PX);
                    self.rebuild_atlas(io);
                }
                Key::Character("n") => {
                    let new = io.open_window(INITIAL_TITLE.to_string(), INITIAL_SIZE);
                    self.windows.insert(new);
                }
                _ => {}
            },
            _ => {}
        }
    }

    pub fn tick(&mut self, _dt: f64, _io: &mut dyn IO) {
        // Future: cursor blink, scroll inertia, anything else that
        // depends on elapsed time goes here.
    }

    pub fn draw(&self, window: WindowId, drawing: &mut Drawing) {
        assert!(self.windows.contains(&window));

        let clip = Rect {
            x: TEXT_X - 4.0,
            y: TEXT_Y - 4.0,
            w: 180.0,
            h: self.atlas.cell_h as f32 + 8.0,
        };
        drawing.push_clip_rect(clip);
        drawing.draw_rect(&self.atlas, clip, HIGHLIGHT_COLOR);
        let label = format!("window {:?}", window);
        drawing.draw_text(
            &self.atlas,
            label.as_str().into(),
            TEXT_X,
            TEXT_Y,
            TEXT_COLOR,
        );
        drawing.pop_clip_rect();
    }

    fn rebuild_atlas(&mut self, io: &mut dyn IO) {
        self.atlas = Atlas::build(&self.font, self.px_size);
        io.reload_atlas(&self.atlas);
    }
}
