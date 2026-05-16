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
//                    into screen-space quads.
//
// Per-frame drawing recipe (modelled on the Zig editor on master):
//
//   1. one RGBA texture holds every glyph as (255,255,255,alpha) plus a
//      solid-white texel for flat-colored rectangles;
//   2. every frame, the CPU builds a fresh list of quads — text + UI rects
//      — into a vertex array (position, UV, color per vertex);
//   3. that vertex array is re-uploaded into a VBO each frame;
//   4. one shader, one draw call: the fragment shader multiplies the sampled
//      texel by the per-vertex color, so the same draw handles tinted text
//      and solid rectangles uniformly.
//
// Rebuilding geometry every frame is fine for an editor — there's no draw
// at all unless something changed (ControlFlow::Wait), and editor content
// depends on cursor / scroll / layout that can't be usefully cached.

use std::ffi::CString;
use std::mem;
use std::num::NonZeroU32;
use std::ptr;

use focus::text::{Atlas, DrawCommand, Frame, Quad, Rect};
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

// =====================================================================
// Shaders
// =====================================================================
//
// Two stages, as required: vertex (one invocation per vertex; writes
// gl_Position in clip space) and fragment (one per pixel covered by the
// assembled triangles; writes the final color).
//
// The vertex shader maps pixel coordinates to clip space [-1, +1] and
// flips y (GL's clip space has +y up; our pixels have +y down). It also
// forwards UV and per-vertex color to the fragment shader, which
// interpolates them across the triangle.
//
// The fragment shader multiplies the sampled atlas texel by the vertex
// color. Because glyph texels are (1, 1, 1, alpha):
//   text color = c * (1,1,1,a) = (c.rgb, c.a*a)
// and the white_rect texel is (1, 1, 1, 1):
//   solid fill = c * (1,1,1,1) = c
// So the same shader serves both cases.

const VERT_SRC: &str = r#"
#version 330 core
layout(location=0) in vec2 a_pos;
layout(location=1) in vec2 a_uv;
layout(location=2) in vec4 a_color;
out vec2 v_uv;
out vec4 v_color;
uniform vec2 u_screen;
void main() {
    vec2 p = a_pos / u_screen * 2.0 - 1.0;
    p.y = -p.y;
    gl_Position = vec4(p, 0.0, 1.0);
    v_uv = a_uv;
    v_color = a_color;
}
"#;

const FRAG_SRC: &str = r#"
#version 330 core
in vec2 v_uv;
in vec4 v_color;
out vec4 frag;
uniform sampler2D u_atlas;
void main() {
    frag = v_color * texture(u_atlas, v_uv);
}
"#;

unsafe fn compile_shader(src: &str, kind: u32) -> u32 {
    unsafe {
        let s = gl::CreateShader(kind);
        let csrc = CString::new(src).unwrap();
        gl::ShaderSource(s, 1, &csrc.as_ptr(), ptr::null());
        gl::CompileShader(s);
        let mut ok = 0i32;
        gl::GetShaderiv(s, gl::COMPILE_STATUS, &mut ok);
        if ok == 0 {
            let mut len = 0i32;
            gl::GetShaderiv(s, gl::INFO_LOG_LENGTH, &mut len);
            let mut log = vec![0u8; len as usize];
            gl::GetShaderInfoLog(s, len, ptr::null_mut(), log.as_mut_ptr() as *mut i8);
            panic!("shader compile failed: {}", String::from_utf8_lossy(&log));
        }
        s
    }
}

unsafe fn link_program(vs: u32, fs: u32) -> u32 {
    unsafe {
        let p = gl::CreateProgram();
        gl::AttachShader(p, vs);
        gl::AttachShader(p, fs);
        gl::LinkProgram(p);
        let mut ok = 0i32;
        gl::GetProgramiv(p, gl::LINK_STATUS, &mut ok);
        if ok == 0 {
            panic!("link failed");
        }
        p
    }
}

// =====================================================================
// Vertex data
// =====================================================================
//
// GL 3.3 core has no quads, only triangles. A rectangle becomes two
// triangles sharing a diagonal — six vertices total per quad.
//
// Each vertex is laid out as a C struct so we can hand a Vec<Vertex>
// straight to glBufferData and tell GL the byte offsets of each attribute.
//   pos    : 2 floats     offset 0     (location 0)
//   uv     : 2 floats     offset 8     (location 1)
//   color  : 4 bytes      offset 16    (location 2, normalized)
// Total stride: 20 bytes.

#[repr(C)]
#[derive(Clone, Copy)]
struct Vertex {
    pos: [f32; 2],
    uv: [f32; 2],
    color: [u8; 4],
}

// Append the 6 vertices for one Quad to `out`. UVs are normalized against
// the atlas size; the per-vertex color matches the quad's color.
fn extend_quad(out: &mut Vec<Vertex>, q: &Quad, atlas: &Atlas) {
    let atlas_w = atlas.width as f32;
    let atlas_h = atlas.height as f32;
    let x0 = q.dst_x;
    let y0 = q.dst_y;
    let x1 = x0 + q.dst_w;
    let y1 = y0 + q.dst_h;
    let u0 = q.src_x as f32 / atlas_w;
    let v0 = q.src_y as f32 / atlas_h;
    let u1 = (q.src_x + q.src_w) as f32 / atlas_w;
    let v1 = (q.src_y + q.src_h) as f32 / atlas_h;
    let c = q.color;
    // triangle A: top-left, top-right, bottom-right
    out.push(Vertex {
        pos: [x0, y0],
        uv: [u0, v0],
        color: c,
    });
    out.push(Vertex {
        pos: [x1, y0],
        uv: [u1, v0],
        color: c,
    });
    out.push(Vertex {
        pos: [x1, y1],
        uv: [u1, v1],
        color: c,
    });
    // triangle B: top-left, bottom-right, bottom-left
    out.push(Vertex {
        pos: [x0, y0],
        uv: [u0, v0],
        color: c,
    });
    out.push(Vertex {
        pos: [x1, y1],
        uv: [u1, v1],
        color: c,
    });
    out.push(Vertex {
        pos: [x0, y1],
        uv: [u0, v1],
        color: c,
    });
}

// One contiguous run of vertices that share a scissor rect.
struct Batch {
    clip: Rect,
    offset: usize,
    count: usize,
}

// glScissor uses pixel coords with bottom-left origin, so we flip y from
// our top-left convention. Inputs are floored/ceiled to integers.
unsafe fn set_scissor(rect: Rect, fb_height: i32) {
    let x = rect.x.floor() as i32;
    let y = rect.y.floor() as i32;
    let w = rect.w.ceil() as i32;
    let h = rect.h.ceil() as i32;
    unsafe {
        gl::Scissor(x, fb_height - (y + h), w.max(0), h.max(0));
    }
}

// =====================================================================
// GL object setup
// =====================================================================

struct GlState {
    program: u32,  // the linked vertex+fragment program
    vao: u32,      // vertex array object — caches the attribute layout
    vbo: u32,      // vertex buffer — re-uploaded every frame
    tex: u32,      // the atlas texture — re-uploaded on font-size change
    u_screen: i32, // uniform locations (looked up by name once)
    u_atlas: i32,
}

// init_gl assumes a current GL context. It creates the shader program, the
// VAO/VBO and the atlas texture object, sets up attribute pointers, and
// configures blending + clear color. No atlas pixels and no vertex bytes
// uploaded here — that happens in upload_atlas / per frame.
unsafe fn init_gl() -> GlState {
    unsafe {
        // --- shader program ---
        let vs = compile_shader(VERT_SRC, gl::VERTEX_SHADER);
        let fs = compile_shader(FRAG_SRC, gl::FRAGMENT_SHADER);
        let program = link_program(vs, fs);
        gl::DeleteShader(vs);
        gl::DeleteShader(fs);

        let name_screen = CString::new("u_screen").unwrap();
        let name_atlas = CString::new("u_atlas").unwrap();
        let u_screen = gl::GetUniformLocation(program, name_screen.as_ptr());
        let u_atlas = gl::GetUniformLocation(program, name_atlas.as_ptr());

        // --- atlas texture (handle + parameters only, no data yet) ---
        let mut tex = 0u32;
        gl::GenTextures(1, &mut tex);
        gl::BindTexture(gl::TEXTURE_2D, tex);
        // NEAREST sampling: don't interpolate between texels. Glyphs are drawn
        // 1:1 (one source texel = one screen pixel), so any filtering would
        // just blur the text. Solid rects pull from a 1×1 region so filtering
        // wouldn't matter for them either way.
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::NEAREST as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::NEAREST as i32);
        // CLAMP_TO_EDGE: sampling outside [0,1] returns the nearest edge texel
        // rather than wrapping. Avoids glyphs bleeding into neighbours if a UV
        // ever lands just outside its cell due to float rounding.
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32);

        // --- vertex array + buffer (handles + attribute layout only) ---
        let mut vao = 0u32;
        gl::GenVertexArrays(1, &mut vao);
        gl::BindVertexArray(vao);
        let mut vbo = 0u32;
        gl::GenBuffers(1, &mut vbo);
        gl::BindBuffer(gl::ARRAY_BUFFER, vbo);

        // Vertex attribute layout — matches the #[repr(C)] Vertex struct.
        let stride = mem::size_of::<Vertex>() as i32;
        gl::EnableVertexAttribArray(0);
        gl::VertexAttribPointer(0, 2, gl::FLOAT, gl::FALSE, stride, ptr::null());
        gl::EnableVertexAttribArray(1);
        gl::VertexAttribPointer(1, 2, gl::FLOAT, gl::FALSE, stride, 8usize as *const _);
        // Color is 4 bytes, normalized (0..=255 → 0.0..=1.0 in the shader).
        gl::EnableVertexAttribArray(2);
        gl::VertexAttribPointer(
            2,
            4,
            gl::UNSIGNED_BYTE,
            gl::TRUE,
            stride,
            16usize as *const _,
        );

        // --- alpha blending ---
        //
        // Fragment output is `vertex_color * sampled_texel`. For glyphs, the
        // sampled texel is (1,1,1,a), so output is (c.rgb, c.a*a) with the
        // glyph's coverage in alpha. SRC_ALPHA / ONE_MINUS_SRC_ALPHA then
        // mixes that into the destination — anti-aliased edges become a
        // proportional blend of the text color and the background.
        gl::Enable(gl::BLEND);
        gl::BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);
        gl::ClearColor(1.0, 1.0, 1.0, 1.0);

        // Scissor test stays on for the whole frame. SetClip commands just
        // move the scissor rect; "no clip" is just a scissor matching the
        // full framebuffer.
        gl::Enable(gl::SCISSOR_TEST);

        GlState {
            program,
            vao,
            vbo,
            tex,
            u_screen,
            u_atlas,
        }
    }
}

// Replace the atlas pixels on the GPU. Called once initially and again
// whenever the font size changes. glTexImage2D reallocates storage, so a
// new size is fine — the texture handle is unchanged.
unsafe fn upload_atlas(gl_state: &GlState, atlas: &Atlas) {
    unsafe {
        gl::BindTexture(gl::TEXTURE_2D, gl_state.tex);
        // Atlas is RGBA8 = 4 bytes per texel, so row pitch is always
        // 4-aligned and the default UNPACK_ALIGNMENT of 4 is fine.
        gl::TexImage2D(
            gl::TEXTURE_2D,
            0, // mip level (we have no mipmaps)
            gl::RGBA8 as i32,
            atlas.width as i32,
            atlas.height as i32,
            0, // border, must be 0
            gl::RGBA,
            gl::UNSIGNED_BYTE,
            atlas.pixels.as_ptr() as *const _,
        );
    }
}

// =====================================================================
// Window + event loop (winit 0.30 / glutin 0.32)
// =====================================================================

struct State {
    window: Window,
    surface: Surface<WindowSurface>,
    context: PossiblyCurrentContext,
    gl: GlState,
    font_bytes: Vec<u8>,
    px_size: f32,
    atlas: Atlas,
}

const TEXT: &str = "hello world";
const TEXT_X: f32 = 20.0;
const TEXT_Y: f32 = 20.0;
const INITIAL_PX: f32 = 32.0;
const MIN_PX: f32 = 4.0;

// Colors. Stored as RGBA8; alpha=255 means opaque.
const TEXT_COLOR: [u8; 4] = [30, 30, 40, 255];
const HIGHLIGHT_COLOR: [u8; 4] = [255, 240, 170, 255];

impl State {
    // Rebuild the atlas at the current px_size and re-upload it to the
    // GPU. The vertex buffer doesn't need touching here — it's rebuilt
    // every frame anyway — but we do need to ask for a redraw, since
    // ControlFlow::Wait means nothing happens otherwise.
    fn rebuild_atlas(&mut self) {
        self.atlas = Atlas::build(&self.font_bytes, self.px_size);
        unsafe { upload_atlas(&self.gl, &self.atlas) };
        self.window.request_redraw();
    }

    // Build this frame's command list, upload one vertex buffer, draw
    // each clip region.
    fn draw(&mut self) {
        let size = self.window.inner_size();
        let fb_w = size.width as f32;
        let fb_h = size.height as f32;

        // ---- assemble commands ----
        let mut frame = Frame::new(fb_w, fb_h);

        // A clip rect deliberately narrower than the text — the right side
        // of "hello world" will get scissored off.
        let clip = Rect::new(
            TEXT_X - 4.0,
            TEXT_Y - 4.0,
            180.0,
            self.atlas.line_height - 8.0,
        );
        frame.push_clip_rect(clip);
        // The highlight is the clip box itself — software-trimmed inside
        // draw_rect, so it never overflows.
        frame.draw_rect(&self.atlas, clip, HIGHLIGHT_COLOR);
        // Text overflows the right edge of the clip; the SetClip / scissor
        // pair around it cuts the trailing glyphs at their pixel edges.
        frame.draw_text(&self.atlas, TEXT, TEXT_X, TEXT_Y, TEXT_COLOR);
        frame.pop_clip_rect();

        // ---- build batches: one vertex stream, one draw per scissor region ----
        let mut all_verts: Vec<Vertex> = Vec::new();
        let mut batches: Vec<Batch> = Vec::new();
        let mut current_clip = Rect::new(0.0, 0.0, fb_w, fb_h);
        let mut batch_start = 0usize;

        for cmd in frame.commands() {
            match cmd {
                DrawCommand::Quad(q) => extend_quad(&mut all_verts, q, &self.atlas),
                DrawCommand::SetClip(r) => {
                    if all_verts.len() > batch_start {
                        batches.push(Batch {
                            clip: current_clip,
                            offset: batch_start,
                            count: all_verts.len() - batch_start,
                        });
                        batch_start = all_verts.len();
                    }
                    current_clip = *r;
                }
            }
        }
        if all_verts.len() > batch_start {
            batches.push(Batch {
                clip: current_clip,
                offset: batch_start,
                count: all_verts.len() - batch_start,
            });
        }

        // ---- upload + draw ----
        unsafe {
            gl::Viewport(0, 0, size.width as i32, size.height as i32);
            // Clear ignores the scissor only if we set it to the full
            // framebuffer first — otherwise the clear is itself scissored.
            gl::Scissor(0, 0, size.width as i32, size.height as i32);
            gl::Clear(gl::COLOR_BUFFER_BIT);

            gl::BindBuffer(gl::ARRAY_BUFFER, self.gl.vbo);
            gl::BufferData(
                gl::ARRAY_BUFFER,
                (all_verts.len() * mem::size_of::<Vertex>()) as isize,
                all_verts.as_ptr() as *const _,
                gl::STREAM_DRAW,
            );

            gl::UseProgram(self.gl.program);
            gl::Uniform2f(self.gl.u_screen, fb_w, fb_h);
            gl::ActiveTexture(gl::TEXTURE0);
            gl::BindTexture(gl::TEXTURE_2D, self.gl.tex);
            gl::Uniform1i(self.gl.u_atlas, 0);
            gl::BindVertexArray(self.gl.vao);

            for batch in &batches {
                set_scissor(batch.clip, size.height as i32);
                gl::DrawArrays(gl::TRIANGLES, batch.offset as i32, batch.count as i32);
            }
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

        let gl_state = unsafe { init_gl() };

        let font_bytes = std::fs::read("deps/FiraCode-Regular.ttf").unwrap();
        let atlas = Atlas::build(&font_bytes, INITIAL_PX);

        let state = State {
            window,
            surface,
            context,
            gl: gl_state,
            font_bytes,
            px_size: INITIAL_PX,
            atlas,
        };
        unsafe { upload_atlas(&state.gl, &state.atlas) };
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
