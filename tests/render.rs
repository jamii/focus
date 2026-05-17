// Offscreen render test.
//
// Brings up an EGL context with no display server (the same libEGL +
// llvmpipe stack that backs hardware GL on Linux), renders a frame via
// the real Renderer into a pbuffer surface, reads the pixels back, and
// writes a PNG. So the test exercises the shaders, blending, scissor —
// not a parallel CPU simulation.

use std::ffi::CString;
use std::fs;
use std::io::BufWriter;
use std::path::Path;
use std::ptr;

use focus::render::Renderer;
use focus::text::{Atlas, Drawing, Rect};
use fontdue::{Font, FontSettings};
use khronos_egl::{self as egl, DynamicInstance};

const W: u32 = 400;
const H: u32 = 80;

fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) {
    let file = fs::File::create(path).unwrap();
    let w = BufWriter::new(file);
    let mut encoder = png::Encoder::new(w, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(rgba).unwrap();
}

// glReadPixels returns rows bottom-up; PNG (and our screen-space y) is
// top-down. Flip in place.
fn flip_rows(pixels: &mut [u8], width: u32, height: u32) {
    let row = (width * 4) as usize;
    let (top, bot) = pixels.split_at_mut(row * (height as usize / 2));
    for (a, b) in top.chunks_exact_mut(row).zip(bot.rchunks_exact_mut(row)) {
        a.swap_with_slice(b);
    }
}

fn atlas_to_debug_rgba(atlas: &Atlas) -> Vec<u8> {
    let mut out = vec![0u8; atlas.pixels.len() * 4];
    for (i, &a) in atlas.pixels.iter().enumerate() {
        let inv = 255 - a;
        out[i * 4] = inv;
        out[i * 4 + 1] = inv;
        out[i * 4 + 2] = inv;
        out[i * 4 + 3] = 255;
    }
    out
}

// Bring up an EGL display + OpenGL 3.3 Core context backed by a pbuffer
// surface of the requested size. Loads `gl::*` function pointers via EGL.
// Returns the context-bound EGL instance/display/surface/context so the
// caller can keep them alive for the duration of the test.
struct EglHost {
    egl: DynamicInstance<egl::EGL1_5>,
    display: egl::Display,
    surface: egl::Surface,
    context: egl::Context,
}

// Mesa's surfaceless platform — lets us bring up a GL context without any
// display server. Defined by EGL_MESA_platform_surfaceless.
const PLATFORM_SURFACELESS_MESA: egl::Enum = 0x31DD;

impl EglHost {
    fn new(width: u32, height: u32) -> Self {
        let egl = unsafe {
            DynamicInstance::<egl::EGL1_5>::load_required()
                .expect("failed to load libEGL.so >= 1.5 — is it on LD_LIBRARY_PATH?")
        };

        // Use the Mesa surfaceless platform so we don't need a Wayland/X11
        // display. Mesa's llvmpipe takes over for the actual rendering.
        let display = unsafe {
            egl.get_platform_display(
                PLATFORM_SURFACELESS_MESA,
                egl::DEFAULT_DISPLAY,
                &[egl::ATTRIB_NONE],
            )
            .expect("eglGetPlatformDisplay(SURFACELESS_MESA) failed")
        };
        egl.initialize(display).expect("eglInitialize failed");
        egl.bind_api(egl::OPENGL_API).expect("bind opengl");

        let cfg_attrs = [
            egl::SURFACE_TYPE,
            egl::PBUFFER_BIT,
            egl::RED_SIZE,
            8,
            egl::GREEN_SIZE,
            8,
            egl::BLUE_SIZE,
            8,
            egl::ALPHA_SIZE,
            8,
            egl::RENDERABLE_TYPE,
            egl::OPENGL_BIT,
            egl::NONE,
        ];
        let config = egl
            .choose_first_config(display, &cfg_attrs)
            .expect("eglChooseConfig failed")
            .expect("no matching EGL config (no GL drivers in env?)");

        let ctx_attrs = [
            egl::CONTEXT_MAJOR_VERSION,
            3,
            egl::CONTEXT_MINOR_VERSION,
            3,
            egl::CONTEXT_OPENGL_PROFILE_MASK,
            egl::CONTEXT_OPENGL_CORE_PROFILE_BIT,
            egl::NONE,
        ];
        let context = egl
            .create_context(display, config, None, &ctx_attrs)
            .expect("eglCreateContext failed");

        let pb_attrs = [
            egl::WIDTH,
            width as i32,
            egl::HEIGHT,
            height as i32,
            egl::NONE,
        ];
        let surface = egl
            .create_pbuffer_surface(display, config, &pb_attrs)
            .expect("eglCreatePbufferSurface failed");

        egl.make_current(display, Some(surface), Some(surface), Some(context))
            .expect("eglMakeCurrent failed");

        // Load gl function pointers via this EGL instance.
        gl::load_with(|name| {
            let cstr = CString::new(name).unwrap();
            egl.get_proc_address(cstr.to_str().unwrap())
                .map(|p| p as *const _)
                .unwrap_or(ptr::null())
        });

        EglHost {
            egl,
            display,
            surface,
            context,
        }
    }
}

impl Drop for EglHost {
    fn drop(&mut self) {
        let _ = self.egl.make_current(self.display, None, None, None);
        let _ = self.egl.destroy_surface(self.display, self.surface);
        let _ = self.egl.destroy_context(self.display, self.context);
        let _ = self.egl.terminate(self.display);
    }
}

#[test]
fn renders_hello_world() {
    let _egl = EglHost::new(W, H);

    let font_bytes = fs::read("deps/FiraCode-Regular.ttf").unwrap();
    let font = Font::from_bytes(font_bytes, FontSettings::default()).unwrap();
    let atlas = Atlas::build(&font, 32.0);
    assert!(atlas.glyphs.contains_key(&'H'));
    assert!(atlas.glyphs.contains_key(&'~'));
    assert!(!atlas.glyphs.contains_key(&'→'));

    let out_dir = Path::new("target/test-output");
    fs::create_dir_all(out_dir).unwrap();
    write_png(
        &out_dir.join("atlas.png"),
        atlas.width,
        atlas.height,
        &atlas_to_debug_rgba(&atlas),
    );

    let mut renderer = unsafe { Renderer::new() };
    unsafe { renderer.upload_atlas(&atlas) };

    let mut drawing = Drawing::new(W as f32, H as f32);
    let clip = Rect {
        x: 16.0,
        y: 16.0,
        w: 180.0,
        h: atlas.cell_h as f32 + 8.0,
    };
    drawing.push_clip_rect(clip);
    drawing.draw_rect(&atlas, clip, [255, 240, 170, 255]);
    // Non-ASCII '→' is not in the atlas and should render as a tofu box.
    drawing.draw_text(
        &atlas,
        "hello → world".into(),
        20.0,
        20.0,
        [30, 30, 40, 255],
    );
    drawing.pop_clip_rect();

    unsafe { renderer.render(drawing.commands(), W as i32, H as i32) };

    let mut pixels = vec![0u8; (W * H * 4) as usize];
    unsafe {
        gl::ReadPixels(
            0,
            0,
            W as i32,
            H as i32,
            gl::RGBA,
            gl::UNSIGNED_BYTE,
            pixels.as_mut_ptr() as *mut _,
        );
    }
    flip_rows(&mut pixels, W, H);
    write_png(&out_dir.join("hello_world.png"), W, H, &pixels);

    let sample = |x: u32, y: u32| {
        let i = ((y * W + x) * 4) as usize;
        (pixels[i], pixels[i + 1], pixels[i + 2])
    };

    // Inside the highlight rect: not pure white.
    let inside = sample(60, 30);
    assert!(
        inside.0 != 255 || inside.1 != 255 || inside.2 != 255,
        "highlight rectangle didn't render"
    );

    // Just right of the clip rect (clip ends at x=16+180=196): scissor
    // and software-trim should leave the background pristine white.
    let outside = sample(220, 30);
    assert_eq!(
        outside,
        (255, 255, 255),
        "pixels outside the clip should be untouched"
    );

    // At least one near-black pixel from the glyph body.
    let dark = pixels
        .chunks_exact(4)
        .any(|p| p[0] < 60 && p[1] < 60 && p[2] < 60);
    assert!(dark, "no text was rendered");
}
