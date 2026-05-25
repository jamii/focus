// Fuzz harness for the editor.
//
// `fuzz_one` drives an empty `App` with a sequence of synthesized
// `InputEvent`s and ticks, both pulled from a `Frng` (see fuzz_gen.rs).
// Time advances by a fuzzer-chosen delta between events. The IO is mocked
// — no winit, no GL — so the harness runs anywhere.
//
// This is the body of the honggfuzz target (see src/bin/fuzz_hfuzz.rs)
// and is also driven from the standalone runner (src/bin/fuzz.rs), which
// finds crashes locally via fuzz_gen::minimize without a honggfuzz
// toolchain.

use std::time::Duration;

use winit::dpi::LogicalSize;
use winit::event::ElementState;
use winit::keyboard::{Key, ModifiersState, NamedKey, SmolStr};

use crate::app::{App, IO, InputEvent, WindowId};
use crate::atlas::Atlas;
use crate::drawing::Drawing;
use crate::fuzz_gen::Frng;

// Mock IO: tracks open windows, fabricates fresh WindowIds, advances
// `frame_start` by whatever the harness pushes via `advance`.
pub struct MockIO {
    next_winit_id: u64,
    pub frame_start: Duration,
    pub open_windows: Vec<WindowId>,
    pub exited: bool,
    pub screen_size: [f32; 2],
}

impl MockIO {
    pub fn new() -> Self {
        MockIO {
            // 1, not 0 — `winit::window::WindowId::dummy()` is 0 on some
            // backends and we don't want to collide with a real id we
            // pretend to mint.
            next_winit_id: 1,
            frame_start: Duration::ZERO,
            open_windows: Vec::new(),
            exited: false,
            screen_size: [0.0, 0.0],
        }
    }

    pub fn fresh_window_id(&mut self) -> WindowId {
        let id = WindowId(winit::window::WindowId::from(self.next_winit_id));
        self.next_winit_id += 1;
        id
    }
}

impl IO for MockIO {
    fn frame_start(&self) -> Duration {
        self.frame_start
    }

    fn open_window(&mut self, _title: String, _size: LogicalSize<u32>) -> WindowId {
        let id = self.fresh_window_id();
        self.open_windows.push(id);
        id
    }

    fn close_window(&mut self, window_id: WindowId) {
        self.open_windows.retain(|w| *w != window_id);
    }

    fn set_window_title(&mut self, _window_id: WindowId, _title: String) {}

    fn request_redraw(&mut self, _window_id: WindowId) {}

    fn reload_atlas(&mut self, _atlas: &Atlas) {}

    fn exit(&mut self) {
        self.exited = true;
    }
}

// Action picked by the fuzzer for each step.
const A_KEY_CHAR: u32 = 40;
const A_KEY_NAMED: u32 = 20;
const A_MODIFIERS: u32 = 10;
const A_CLOSE: u32 = 2;
const A_TICK: u32 = 20;
const A_DRAW: u32 = 40;
const A_SCROLL: u32 = 20;

// Each step: tick once (advancing time), then perform one randomly
// chosen action. Returns Some(()) if more entropy is available; None
// when the buffer is exhausted (Frng signals end-of-stream as None).
fn step(frng: &mut Frng, app: &mut App, io: &mut MockIO) -> Option<()> {
    if io.open_windows.is_empty() {
        return None;
    }
    let window_idx = frng.usize_bounded(0, io.open_windows.len() - 1)?;
    let window_id = io.open_windows[window_idx];

    let action = frng.weighted(&[
        A_KEY_CHAR,
        A_KEY_NAMED,
        A_MODIFIERS,
        A_CLOSE,
        A_TICK,
        A_DRAW,
        A_SCROLL,
    ])?;
    match action {
        0 => {
            // Most of the time, printable ASCII. Occasionally a
            // multi-byte UTF-8 char, to exercise mid-codepoint logic.
            let ch = if frng.u8_bounded(0, 15)? == 0 {
                // Pick from a small set of multi-byte chars.
                match frng.u8_bounded(0, 4)? {
                    0 => 'é', // 2 bytes
                    1 => 'ü',
                    2 => '€',  // 3 bytes
                    3 => '猫', // 3 bytes
                    _ => '🦀', // 4 bytes
                }
            } else {
                frng.u8_bounded(0x20, 0x7e)? as char
            };
            let mut buf = [0u8; 4];
            let s = ch.encode_utf8(&mut buf);
            let event = InputEvent::Key {
                state: ElementState::Pressed,
                logical_key: Key::Character(SmolStr::new(s)),
            };
            app.input(io, window_id, event);
        }
        1 => {
            // Named key (Enter, Space, Backspace, Delete, plus a few
            // others to exercise unhandled-key paths).
            let named = match frng.u8_bounded(0, 7)? {
                0 => NamedKey::Enter,
                1 => NamedKey::Space,
                2 => NamedKey::Backspace,
                3 => NamedKey::Delete,
                4 => NamedKey::ArrowLeft,
                5 => NamedKey::ArrowRight,
                6 => NamedKey::ArrowUp,
                _ => NamedKey::ArrowDown,
            };
            let state = if frng.boolean()? {
                ElementState::Pressed
            } else {
                ElementState::Released
            };
            app.input(
                io,
                window_id,
                InputEvent::Key {
                    state,
                    logical_key: Key::Named(named),
                },
            );
        }
        2 => {
            // Random modifier state: any subset of Ctrl/Alt/Shift/Super.
            let bits = frng.u8_bounded(0, 0x0F)?;
            let mut m = ModifiersState::empty();
            if bits & 0b0001 != 0 {
                m |= ModifiersState::CONTROL;
            }
            if bits & 0b0010 != 0 {
                m |= ModifiersState::ALT;
            }
            if bits & 0b0100 != 0 {
                m |= ModifiersState::SHIFT;
            }
            if bits & 0b1000 != 0 {
                m |= ModifiersState::SUPER;
            }
            app.input(io, window_id, InputEvent::ModifiersChanged(m));
        }
        3 => {
            app.input(io, window_id, InputEvent::CloseRequested);
        }
        4 => {
            // Advance time by a fuzzer-chosen delta in [0, ~1s].
            let delta_us = frng.u32_bounded(0, 1_000_000)?;
            io.frame_start += Duration::from_micros(delta_us as u64);
            app.tick(io);
        }
        5 => {
            // Draw at a fuzzer-chosen screen size.
            if frng.boolean()? {
                io.screen_size = [
                    frng.u32_bounded(0, 4000)? as f32,
                    frng.u32_bounded(0, 4000)? as f32,
                ];
            }
            let mut drawing = Drawing::new(io.screen_size);
            app.draw(window_id, &mut drawing);
        }
        6 => {
            // Mouse wheel scroll. Map the raw byte into a signed scroll
            // amount roughly the size of a wheel notch, with occasional
            // larger jumps (smooth-scroll / pixel-delta sized).
            let raw = frng.u8_bounded(0, 200)? as f32;
            let y_offset = (raw - 100.0) / 10.0;
            app.input(io, window_id, InputEvent::MouseWheel { y_offset });
        }
        _ => unreachable!(),
    }
    Some(())
}

pub fn fuzz_one(bytes: &[u8]) {
    let mut frng = Frng::new(bytes);
    let mut io = MockIO::new();
    let initial = io.fresh_window_id();
    io.open_windows.push(initial);
    let mut app = App::new(initial, &mut io);

    loop {
        if step(&mut frng, &mut app, &mut io).is_none() {
            break;
        }
        if io.exited {
            break;
        }
    }

    app.assert_invariants();
}
