// Pure core: editor state + input/output protocol.
//
// App exposes three operations:
//
//   input  — process a single input event (one window event)
//   tick   — advance time-based state by `dt` seconds
//   draw   — render the current state into a `Drawing`
//
// The split lets the outside choose its loop policy (poll vs wait,
// per-frame vs per-event tick) without state caring. App only knows
// "here's an event" and "here's how much time has passed."

use std::collections::HashSet;
use std::time::Instant;

use winit::dpi::LogicalSize;
use winit::event::{ElementState, WindowEvent};
use winit::keyboard::{Key, NamedKey};

pub use crate::text::{Atlas, Drawing, Rect};
use crate::text::{Font, FontSettings};

// Opaque window handle minted by `App`. `main.rs` maps these to
// whatever the platform calls a window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WindowId(u32);

pub type InputEvent = WindowEvent;

#[derive(Clone, Debug)]
pub enum OutputEvent {
    OpenWindow {
        window: WindowId,
        title: String,
        size: LogicalSize<u32>,
    },
    CloseWindow {
        window: WindowId,
    },
    SetWindowTitle {
        window: WindowId,
        title: String,
    },
    // The atlas pixels changed; main should re-upload before next draw.
    AtlasChanged,
    Exit,
}

// External effects app may need. Currently just a clock; will grow.
pub trait IO {
    fn now(&self) -> Instant;
}

pub struct App {
    next_window_id: u32,
    windows: HashSet<WindowId>,
    font: Font,
    px_size: f32,
    atlas: Atlas,
    // Consumed on the first `tick`: emits the initial `OpenWindow`.
    initial_open_pending: bool,
}

const INITIAL_PX: f32 = 32.0;
const MIN_PX: f32 = 4.0;
const INITIAL_TITLE: &str = "focus";
const INITIAL_SIZE: LogicalSize<u32> = LogicalSize {
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
    pub fn new(font_bytes: Vec<u8>) -> App {
        let font = Font::from_bytes(font_bytes, FontSettings::default()).unwrap();
        let atlas = Atlas::build(&font, INITIAL_PX);
        App {
            next_window_id: 0,
            windows: HashSet::new(),
            font,
            px_size: INITIAL_PX,
            atlas,
            initial_open_pending: true,
        }
    }

    pub fn atlas(&self) -> &Atlas {
        &self.atlas
    }

    fn fresh_window_id(&mut self) -> WindowId {
        let id = WindowId(self.next_window_id);
        self.next_window_id += 1;
        id
    }

    fn rebuild_atlas(&mut self, outputs: &mut Vec<OutputEvent>) {
        self.atlas = Atlas::build(&self.font, self.px_size);
        outputs.push(OutputEvent::AtlasChanged);
    }

    pub fn input(
        &mut self,
        window: WindowId,
        event: InputEvent,
        _io: &dyn IO,
        outputs: &mut Vec<OutputEvent>,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                self.windows.remove(&window);
                outputs.push(OutputEvent::CloseWindow { window });
                if self.windows.is_empty() {
                    outputs.push(OutputEvent::Exit);
                }
            }
            WindowEvent::KeyboardInput {
                event: key_event, ..
            } if key_event.state == ElementState::Pressed => match key_event.logical_key.as_ref() {
                Key::Named(NamedKey::Escape) => outputs.push(OutputEvent::Exit),
                Key::Character("+") => {
                    self.px_size += 1.0;
                    self.rebuild_atlas(outputs);
                }
                Key::Character("-") => {
                    self.px_size = (self.px_size - 1.0).max(MIN_PX);
                    self.rebuild_atlas(outputs);
                }
                Key::Character("n") => {
                    let new = self.fresh_window_id();
                    self.windows.insert(new);
                    outputs.push(OutputEvent::OpenWindow {
                        window: new,
                        title: format!("focus - window {}", new.0),
                        size: INITIAL_SIZE,
                    });
                }
                _ => {}
            },
            _ => {}
        }
    }

    pub fn tick(&mut self, _dt: f64, _io: &dyn IO, outputs: &mut Vec<OutputEvent>) {
        if self.initial_open_pending {
            self.initial_open_pending = false;
            let window = self.fresh_window_id();
            self.windows.insert(window);
            outputs.push(OutputEvent::OpenWindow {
                window,
                title: INITIAL_TITLE.to_string(),
                size: INITIAL_SIZE,
            });
        }
        // Future: cursor blink, scroll inertia, anything else that
        // depends on elapsed time goes here.
    }

    pub fn draw(&self, window: WindowId, drawing: &mut Drawing) {
        if !self.windows.contains(&window) {
            return;
        }

        let clip = Rect {
            x: TEXT_X - 4.0,
            y: TEXT_Y - 4.0,
            w: 180.0,
            h: self.atlas.cell_h as f32 + 8.0,
        };
        drawing.push_clip_rect(clip);
        drawing.draw_rect(&self.atlas, clip, HIGHLIGHT_COLOR);
        let label = format!("window {}", window.0);
        drawing.draw_text(
            &self.atlas,
            label.as_str().into(),
            TEXT_X,
            TEXT_Y,
            TEXT_COLOR,
        );
        drawing.pop_clip_rect();
    }
}
