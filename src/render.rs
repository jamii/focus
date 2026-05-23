// GPU renderer for a list of DrawCommands.
//
// Owns nothing windowing-related — just GL objects (shader program,
// VAO/VBO, atlas texture). Caller supplies a current GL context and the
// framebuffer it should draw into. That makes the renderer reusable
// between the windowed path (drawing into the swap-chain back buffer)
// and the test path (drawing into an offscreen FBO / pbuffer).

use std::ffi::CString;
use std::mem::{self, offset_of};
use std::ptr;

use crate::text::{Atlas, DrawCommand, Rect};

// Two shaders, the minimum required: vertex (one invocation per vertex;
// writes gl_Position in clip space) and fragment (one per pixel covered;
// writes the final color).
//
// The vertex shader maps pixel coordinates to clip space [-1, +1] and
// flips y (GL clip space is +y up; our pixels are +y down). It forwards
// UV and per-vertex color to the fragment shader, which interpolates them
// across the triangle.
//
// The fragment shader multiplies the per-vertex alpha by the sampled
// atlas coverage. The atlas is R8 — each texel stores alpha only; the
// color comes entirely from the vertex. For solid fills (rects), UVs
// point at the dedicated white texel (coverage 1), so alpha is just
// the vertex's alpha and the rect renders flat.

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
    frag = vec4(v_color.rgb, v_color.a * texture(u_atlas, v_uv).r);
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

// Per-vertex: pos (2 floats) + uv (2 floats) + color (4 bytes normalized).
// Each Quad emits 6 vertices (two triangles). Attribute offsets come from
// `offset_of!`, so adding a field to Vertex doesn't desync the GL setup.
#[repr(C)]
#[derive(Clone, Copy)]
struct Vertex {
    pos: [f32; 2],
    uv: [f32; 2],
    color: [u8; 4],
}

// One contiguous run of vertices that share a scissor rect.
struct Batch {
    clip: Rect,
    offset: usize,
    count: usize,
}

// glScissor uses pixel coords with bottom-left origin; flip y from our
// top-left convention.
unsafe fn set_scissor(rect: Rect, fb_height: i32) {
    let x = rect.pos[0].floor() as i32;
    let y = rect.pos[1].floor() as i32;
    let w = rect.size[0].ceil() as i32;
    let h = rect.size[1].ceil() as i32;
    // glScissor rejects negative x/y with GL_INVALID_VALUE; clamp the
    // origin to zero and shrink the size by the amount we clipped so the
    // far edge of the rect stays put.
    let x_clamped = x.max(0);
    let y_top = fb_height - (y + h);
    let y_clamped = y_top.max(0);
    let w_clamped = (w - (x_clamped - x)).max(0);
    let h_clamped = (h - (y_clamped - y_top)).max(0);
    unsafe {
        gl::Scissor(x_clamped, y_clamped, w_clamped, h_clamped);
    }
}

pub struct Renderer {
    // GL object names (opaque ints). They're bound once in `new` and
    // never rebound — the GL state machine keeps using them — but we
    // hold onto the handles so `Drop` can delete them on shutdown.
    program: u32,
    vao: u32,
    vbo: u32,
    // Atlas texture. Also rebound (with new pixels) in `upload_atlas`
    // when the font size changes.
    tex: u32,
    // Atlas dimensions cached from the most recent upload, so `render`
    // can normalize uvs without being handed the atlas every frame.
    atlas_w: u32,
    atlas_h: u32,
    // Uniform location for the `u_screen` (framebuffer size in pixels).
    // The vertex shader divides vertex positions by this to get NDC.
    u_screen: i32,
    // Per-frame scratch, kept across calls to avoid reallocating.
    vertex_buf: Vec<Vertex>,
    batches: Vec<Batch>,
}

impl Renderer {
    /// Create all GL objects. A current GL context must already exist.
    pub unsafe fn new() -> Self {
        unsafe {
            let vs = compile_shader(VERT_SRC, gl::VERTEX_SHADER);
            let fs = compile_shader(FRAG_SRC, gl::FRAGMENT_SHADER);
            let program = link_program(vs, fs);
            gl::DeleteShader(vs);
            gl::DeleteShader(fs);

            let name_screen = CString::new("u_screen").unwrap();
            let name_atlas = CString::new("u_atlas").unwrap();
            let u_screen = gl::GetUniformLocation(program, name_screen.as_ptr());
            let u_atlas = gl::GetUniformLocation(program, name_atlas.as_ptr());

            // Atlas texture (handle + parameters, no data yet).
            let mut tex = 0u32;
            gl::GenTextures(1, &mut tex);
            gl::BindTexture(gl::TEXTURE_2D, tex);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::NEAREST as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::NEAREST as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32);

            // VAO + VBO with the vertex attribute layout.
            let mut vao = 0u32;
            gl::GenVertexArrays(1, &mut vao);
            gl::BindVertexArray(vao);
            let mut vbo = 0u32;
            gl::GenBuffers(1, &mut vbo);
            gl::BindBuffer(gl::ARRAY_BUFFER, vbo);
            let stride = mem::size_of::<Vertex>() as i32;
            gl::EnableVertexAttribArray(0);
            gl::VertexAttribPointer(
                0,
                2,
                gl::FLOAT,
                gl::FALSE,
                stride,
                offset_of!(Vertex, pos) as *const _,
            );
            gl::EnableVertexAttribArray(1);
            gl::VertexAttribPointer(
                1,
                2,
                gl::FLOAT,
                gl::FALSE,
                stride,
                offset_of!(Vertex, uv) as *const _,
            );
            gl::EnableVertexAttribArray(2);
            gl::VertexAttribPointer(
                2,
                4,
                gl::UNSIGNED_BYTE,
                gl::TRUE,
                stride,
                offset_of!(Vertex, color) as *const _,
            );

            // Alpha blending: src.rgb * src.a + dst * (1 - src.a).
            gl::Enable(gl::BLEND);
            gl::BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);
            gl::ClearColor(1.0, 1.0, 1.0, 1.0);

            // Scissor stays on for the whole frame. SetClip just moves the
            // rect; "no clip" means a scissor rect covering the framebuffer.
            gl::Enable(gl::SCISSOR_TEST);

            // The remaining state is set-once: the program is the only one
            // we ever use, the VAO records the attribute layout, the
            // sampler always reads from texture unit 0, and the atlas
            // texture is always bound to that unit. Per-frame `render`
            // therefore only has to update the VBO contents, u_screen, and
            // the scissor rect.
            gl::UseProgram(program);
            gl::Uniform1i(u_atlas, 0);
            gl::ActiveTexture(gl::TEXTURE0);

            Renderer {
                program,
                vao,
                vbo,
                tex,
                atlas_w: 0,
                atlas_h: 0,
                u_screen,
                vertex_buf: Vec::new(),
                batches: Vec::new(),
            }
        }
    }

    /// Replace the atlas pixels on the GPU. Texture handle is unchanged.
    pub unsafe fn upload_atlas(&mut self, atlas: &Atlas) {
        unsafe {
            gl::BindTexture(gl::TEXTURE_2D, self.tex);
            // R8 rows are 1 byte per pixel — set unpack alignment to 1
            // so rows whose width isn't a multiple of 4 are read whole.
            gl::PixelStorei(gl::UNPACK_ALIGNMENT, 1);
            gl::TexImage2D(
                gl::TEXTURE_2D,
                0,
                gl::R8 as i32,
                atlas.size[0] as i32,
                atlas.size[1] as i32,
                0,
                gl::RED,
                gl::UNSIGNED_BYTE,
                atlas.pixels.as_ptr() as *const _,
            );
        }
        self.atlas_w = atlas.size[0];
        self.atlas_h = atlas.size[1];
    }

    /// Render a command list into whatever framebuffer is currently bound.
    ///
    /// Walks the commands once to build a single vertex buffer (one batch
    /// per scissor region), uploads it, then issues one DrawArrays per
    /// batch with the appropriate glScissor. The framebuffer is cleared
    /// to white first.
    pub unsafe fn render(&mut self, commands: &[DrawCommand], fb_w: i32, fb_h: i32) {
        self.vertex_buf.clear();
        self.batches.clear();

        let inv_atlas_w = 1.0 / self.atlas_w as f32;
        let inv_atlas_h = 1.0 / self.atlas_h as f32;
        let mut current_clip = Rect {
            pos: [0.0, 0.0],
            size: [fb_w as f32, fb_h as f32],
        };
        let mut batch_start = 0usize;

        for cmd in commands {
            match cmd {
                DrawCommand::Quad(q) => {
                    let x0 = q.dst_pos[0];
                    let y0 = q.dst_pos[1];
                    let x1 = x0 + q.dst_size[0];
                    let y1 = y0 + q.dst_size[1];
                    let u0 = q.src_pos[0] as f32 * inv_atlas_w;
                    let v0 = q.src_pos[1] as f32 * inv_atlas_h;
                    let u1 = (q.src_pos[0] + q.src_size[0]) as f32 * inv_atlas_w;
                    let v1 = (q.src_pos[1] + q.src_size[1]) as f32 * inv_atlas_h;
                    let c = q.color;
                    // Two triangles per quad: (tl, tr, br) and (tl, br, bl).
                    self.vertex_buf.extend_from_slice(&[
                        Vertex {
                            pos: [x0, y0],
                            uv: [u0, v0],
                            color: c,
                        },
                        Vertex {
                            pos: [x1, y0],
                            uv: [u1, v0],
                            color: c,
                        },
                        Vertex {
                            pos: [x1, y1],
                            uv: [u1, v1],
                            color: c,
                        },
                        Vertex {
                            pos: [x0, y0],
                            uv: [u0, v0],
                            color: c,
                        },
                        Vertex {
                            pos: [x1, y1],
                            uv: [u1, v1],
                            color: c,
                        },
                        Vertex {
                            pos: [x0, y1],
                            uv: [u0, v1],
                            color: c,
                        },
                    ]);
                }
                DrawCommand::SetClip(r) => {
                    if self.vertex_buf.len() > batch_start {
                        self.batches.push(Batch {
                            clip: current_clip,
                            offset: batch_start,
                            count: self.vertex_buf.len() - batch_start,
                        });
                        batch_start = self.vertex_buf.len();
                    }
                    current_clip = *r;
                }
            }
        }
        if self.vertex_buf.len() > batch_start {
            self.batches.push(Batch {
                clip: current_clip,
                offset: batch_start,
                count: self.vertex_buf.len() - batch_start,
            });
        }

        unsafe {
            gl::Viewport(0, 0, fb_w, fb_h);
            // Clear ignores the scissor only if we widen it to the full
            // framebuffer first — otherwise the clear is itself scissored.
            gl::Scissor(0, 0, fb_w, fb_h);
            gl::Clear(gl::COLOR_BUFFER_BIT);

            gl::BufferData(
                gl::ARRAY_BUFFER,
                (self.vertex_buf.len() * mem::size_of::<Vertex>()) as isize,
                self.vertex_buf.as_ptr() as *const _,
                gl::STREAM_DRAW,
            );
            gl::Uniform2f(self.u_screen, fb_w as f32, fb_h as f32);

            for batch in &self.batches {
                set_scissor(batch.clip, fb_h);
                gl::DrawArrays(gl::TRIANGLES, batch.offset as i32, batch.count as i32);
            }
        }
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteProgram(self.program);
            gl::DeleteVertexArrays(1, &self.vao);
            gl::DeleteBuffers(1, &self.vbo);
            gl::DeleteTextures(1, &self.tex);
        }
    }
}
