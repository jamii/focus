use std::fs;
use std::io::BufWriter;
use std::path::Path;

use focus::text::{composite, Atlas, Frame, Rect};

fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) {
    let file = fs::File::create(path).unwrap();
    let w = BufWriter::new(file);
    let mut encoder = png::Encoder::new(w, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(rgba).unwrap();
}

fn atlas_to_debug_rgba(atlas: &Atlas) -> Vec<u8> {
    let mut out = vec![0u8; atlas.pixels.len()];
    for i in 0..(atlas.width * atlas.height) as usize {
        let a = atlas.pixels[i * 4 + 3];
        let inv = 255 - a;
        out[i * 4] = inv;
        out[i * 4 + 1] = inv;
        out[i * 4 + 2] = inv;
        out[i * 4 + 3] = 255;
    }
    out
}

#[test]
fn renders_hello_world() {
    let font_bytes = fs::read("deps/FiraCode-Regular.ttf").unwrap();
    let atlas = Atlas::build(&font_bytes, 32.0);

    assert!(atlas.glyphs.contains_key(&'H'));
    assert!(atlas.glyphs.contains_key(&'~'));

    let out_dir = Path::new("target/test-output");
    fs::create_dir_all(out_dir).unwrap();
    write_png(
        &out_dir.join("atlas.png"),
        atlas.width,
        atlas.height,
        &atlas_to_debug_rgba(&atlas),
    );

    let width = 400u32;
    let height = 80u32;

    let mut frame = Frame::new(width as f32, height as f32);
    let clip = Rect::new(16.0, 16.0, 180.0, atlas.line_height + 8.0);
    frame.push_clip_rect(clip);
    frame.draw_rect(&atlas, clip, [255, 240, 170, 255]);
    frame.draw_text(&atlas, "hello world", 20.0, 20.0, [30, 30, 40, 255]);
    frame.pop_clip_rect();

    let pixels = composite(&atlas, frame.commands(), width, height);
    write_png(&out_dir.join("hello_world.png"), width, height, &pixels);

    // Highlight body should have rendered as yellow-ish (non-white).
    let sample = |x: u32, y: u32| {
        let i = ((y * width + x) * 4) as usize;
        (pixels[i], pixels[i + 1], pixels[i + 2])
    };
    let inside = sample(60, 30);
    assert!(
        inside.0 != 255 || inside.1 != 255 || inside.2 != 255,
        "highlight rectangle didn't render"
    );

    // Just to the right of the clip rect (clip ends at x=16+180=196), the
    // background must still be pristine white — scissor worked.
    let outside = sample(220, 30);
    assert_eq!(
        outside,
        (255, 255, 255),
        "pixels outside the clip should be untouched"
    );

    // At least one dark glyph pixel inside the clip.
    let dark = pixels
        .chunks_exact(4)
        .any(|p| p[0] < 60 && p[1] < 60 && p[2] < 60);
    assert!(dark, "no text was rendered");
}
