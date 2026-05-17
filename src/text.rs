// Glyph atlas + text layout + draw-command list.
//
// The model is borrowed from rxi's microui:
//   * the "editor" side builds a Frame: a sequence of DrawCommands made of
//     Quads (colored textured rectangles) interleaved with SetClip commands
//     (axis-aligned scissor rects);
//   * the renderer (see crate::render) walks the commands in order,
//     batching consecutive Quads into a single draw call and switching
//     scissor whenever it hits a SetClip.
//
// Clipping itself is hybrid:
//   * draw_rect intersects in software — trimming a rectangle to its clip
//     is a single rect-intersection, much cheaper than emitting a scissor
//     change, and it lets the rect stay in the current batch;
//   * draw_text emits a SetClip if (and only if) the text bounding box is
//     partially outside the current clip, then a SetClip back to "no clip"
//     after the glyph quads.
//
// The atlas is RGBA8 with every glyph stored as (255, 255, 255, alpha) plus
// a single solid-white texel. Combined with the renderer's per-vertex color
// modulation, one shader handles tinted glyphs and flat-color rects.

use std::collections::HashMap;

pub use bstr::{BStr, ByteSlice};
pub use fontdue::{Font, FontSettings};

// We only handle printable ASCII for now — code points 32 (' ') through
// 126 ('~'). 95 characters total.
const ASCII_FIRST: u32 = 32;
const ASCII_LAST: u32 = 126;
const NUM_GLYPHS: usize = (ASCII_LAST - ASCII_FIRST + 1) as usize;
const ATLAS_COLS: u32 = 16;

#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

// Axis-aligned intersection. Returns a rect with non-positive w/h when the
// inputs don't overlap; callers check w > 0 && h > 0 before drawing.
pub fn intersect_rects(a: Rect, b: Rect) -> Rect {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = (a.x + a.w).min(b.x + b.w);
    let y1 = (a.y + a.h).min(b.y + b.h);
    Rect {
        x: x0,
        y: y0,
        w: (x1 - x0).max(0.0),
        h: (y1 - y0).max(0.0),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ClipState {
    /// Primitive is fully inside the clip — draw as-is.
    Inside,
    /// Primitive overlaps the clip edge — needs the GPU scissor.
    Partial,
    /// Primitive is entirely outside the clip — skip.
    Outside,
}

fn classify_clip(rect: Rect, clip: Rect) -> ClipState {
    if rect.x >= clip.x + clip.w
        || rect.x + rect.w <= clip.x
        || rect.y >= clip.y + clip.h
        || rect.y + rect.h <= clip.y
    {
        return ClipState::Outside;
    }
    if rect.x >= clip.x
        && rect.x + rect.w <= clip.x + clip.w
        && rect.y >= clip.y
        && rect.y + rect.h <= clip.y + clip.h
    {
        return ClipState::Inside;
    }
    ClipState::Partial
}

// A glyph is just the top-left of its cell in the atlas. Each cell is
// `cell_w × cell_h` texels, with the glyph pre-positioned at its
// baseline-relative offset within that cell — so drawing a character is
// just "blit the whole cell at (pen_x, y)". No per-glyph bearings.
#[derive(Clone, Copy)]
pub struct Glyph {
    pub atlas_x: u32,
    pub atlas_y: u32,
}

pub struct Atlas {
    // R8 texture, laid out row-major. Each texel stores coverage alpha
    // ∈ [0, 255]; the fragment shader supplies the color. The white_rect
    // texel is just 255 (= "fully opaque").
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,

    pub glyphs: HashMap<char, Glyph>,

    // Synthetic hollow-rectangle glyph used as a "tofu" for any
    // character not in `glyphs`. Same role as TTF's glyph index 0.
    pub missing: Glyph,

    // Coordinates of a single solid-white texel. Lives in its own
    // dedicated cell that no glyph quad ever samples; draw_rect points
    // at it for flat-colored fills.
    pub white_x: u32,
    pub white_y: u32,

    // Cell dimensions, in texels = screen pixels (we draw 1:1). cell_w
    // is the rounded-up advance width; cell_h is the rounded-up
    // line_height so a single cell fits any glyph from ascent to descent.
    pub cell_w: u32,
    pub cell_h: u32,
}

impl Atlas {
    pub fn build(font: &Font, px_size: f32) -> Self {
        let line = font.horizontal_line_metrics(px_size).unwrap();
        let ascent = line.ascent;

        // Rasterize all printable ASCII; collect bitmaps + metrics and
        // find the constant advance width.
        let mut raster: Vec<(char, fontdue::Metrics, Vec<u8>)> = Vec::with_capacity(NUM_GLYPHS);
        let mut max_advance = 0.0_f32;
        for cp in ASCII_FIRST..=ASCII_LAST {
            let ch = char::from_u32(cp).unwrap();
            let (m, bitmap) = font.rasterize(ch, px_size);
            max_advance = max_advance.max(m.advance_width);
            raster.push((ch, m, bitmap));
        }

        let cell_w = max_advance.ceil() as u32;
        let cell_h = line.new_line_size.ceil() as u32;

        // Layout: ASCII glyphs, then tofu, then white texel. Each lives
        // in its own cell. 95 + 1 + 1 = 97 cells = 7 rows × 16 cols with
        // 15 unused cells at the end.
        let tofu_idx = NUM_GLYPHS as u32;
        let white_idx = tofu_idx + 1;
        let rows = (white_idx + 1).div_ceil(ATLAS_COLS);
        let atlas_w = ATLAS_COLS * cell_w;
        let atlas_h = rows * cell_h;
        let mut pixels = vec![0u8; (atlas_w * atlas_h) as usize];

        let cell_origin = |idx: u32| ((idx % ATLAS_COLS) * cell_w, (idx / ATLAS_COLS) * cell_h);

        // For each glyph, paint its bitmap into its cell at the
        // bearing-offset that puts it on the cell's baseline. The cell's
        // baseline is at y = ascent from the top:
        //   bitmap_top_in_cell  = ascent - (ymin + height)
        //                       = ascent - distance from baseline to top of bitmap
        //   bitmap_left_in_cell = xmin (fontdue's left side bearing)
        let mut glyphs = HashMap::with_capacity(NUM_GLYPHS);
        for (i, (ch, m, bitmap)) in raster.iter().enumerate() {
            let (cell_x, cell_y) = cell_origin(i as u32);
            let bitmap_left = m.xmin;
            let bitmap_top = ascent as i32 - (m.ymin + m.height as i32);
            let gw = m.width as i32;
            let gh = m.height as i32;

            for j in 0..gh {
                for k in 0..gw {
                    let cx = bitmap_left + k;
                    let cy = bitmap_top + j;
                    // Clamp glyphs that overhang their cell (not
                    // expected for printable ASCII, but defensive).
                    if cx < 0 || cy < 0 || cx >= cell_w as i32 || cy >= cell_h as i32 {
                        continue;
                    }
                    let dst = ((cell_y + cy as u32) * atlas_w + (cell_x + cx as u32)) as usize;
                    pixels[dst] = bitmap[(j * gw + k) as usize];
                }
            }

            glyphs.insert(
                *ch,
                Glyph {
                    atlas_x: cell_x,
                    atlas_y: cell_y,
                },
            );
        }

        // Tofu: hollow rectangle painted into its own cell, with margins
        // so the box visually sits in the cap-letter region.
        let (tx, ty) = cell_origin(tofu_idx);
        let margin_x = (cell_w / 8).max(1);
        let margin_y = (cell_h / 5).max(2);
        let tw = cell_w - 2 * margin_x;
        let th = cell_h - 2 * margin_y;
        for j in 0..th {
            for k in 0..tw {
                let on_border = j == 0 || j == th - 1 || k == 0 || k == tw - 1;
                if on_border {
                    let dst = ((ty + margin_y + j) * atlas_w + (tx + margin_x + k)) as usize;
                    pixels[dst] = 255;
                }
            }
        }
        let missing = Glyph {
            atlas_x: tx,
            atlas_y: ty,
        };

        // White texel in its own cell — never sampled as part of a glyph
        // quad, so it doesn't pollute the tofu render.
        let (white_x, white_y) = cell_origin(white_idx);
        pixels[(white_y * atlas_w + white_x) as usize] = 255;

        Atlas {
            width: atlas_w,
            height: atlas_h,
            pixels,
            glyphs,
            missing,
            white_x,
            white_y,
            cell_w,
            cell_h,
        }
    }
}

// A rectangle to draw: `dst_*` is screen-space (in pixels), `src_*` is
// atlas-space (in texels), `color` is the per-vertex color modulated
// into the sampled texel.
//
//   text glyph  → src is a glyph cell,    color is text color
//   solid fill  → src is the 1×1 white,   color is fill color
#[derive(Clone, Copy, Debug)]
pub struct Quad {
    pub dst_x: f32,
    pub dst_y: f32,
    pub dst_w: f32,
    pub dst_h: f32,
    pub src_x: u32,
    pub src_y: u32,
    pub src_w: u32,
    pub src_h: u32,
    pub color: [u8; 4],
}

#[derive(Clone, Debug)]
pub enum DrawCommand {
    Quad(Quad),
    SetClip(Rect),
}

// A window's worth of drawing, built up by the "editor" and consumed by
// the renderer. Maintains a clip-rect stack; each push intersects with
// the current top, so pushing only ever shrinks the clip.
//
// clip_stack[0] is the full screen rect — also serves as the "no clip"
// sentinel emitted as SetClip after a partially-clipped text draw to
// release the scissor.
pub struct Drawing {
    commands: Vec<DrawCommand>,
    clip_stack: Vec<Rect>,
}

impl Drawing {
    pub fn new(screen_w: f32, screen_h: f32) -> Self {
        Self {
            commands: Vec::new(),
            clip_stack: vec![Rect {
                x: 0.0,
                y: 0.0,
                w: screen_w,
                h: screen_h,
            }],
        }
    }

    pub fn commands(&self) -> &[DrawCommand] {
        &self.commands
    }

    fn current_clip(&self) -> Rect {
        *self.clip_stack.last().unwrap()
    }

    /// Push a new clip, intersected with the current top of the stack.
    /// Subsequent draws will be clipped to this rectangle.
    pub fn push_clip_rect(&mut self, rect: Rect) {
        let top = self.current_clip();
        self.clip_stack.push(intersect_rects(rect, top));
    }

    pub fn pop_clip_rect(&mut self) {
        self.clip_stack.pop();
        assert!(
            !self.clip_stack.is_empty(),
            "popped the initial (screen) clip"
        );
    }

    /// Solid rectangle. Trimmed against the current clip in software, so
    /// it doesn't need to break the batch.
    pub fn draw_rect(&mut self, atlas: &Atlas, rect: Rect, color: [u8; 4]) {
        let trimmed = intersect_rects(rect, self.current_clip());
        if trimmed.w > 0.0 && trimmed.h > 0.0 {
            self.commands.push(DrawCommand::Quad(Quad {
                dst_x: trimmed.x,
                dst_y: trimmed.y,
                dst_w: trimmed.w,
                dst_h: trimmed.h,
                src_x: atlas.white_x,
                src_y: atlas.white_y,
                src_w: 1,
                src_h: 1,
                color,
            }));
        }
    }

    /// Text, clipped via GL scissor if necessary. (x, y) is the top-left
    /// of the line's bounding box.
    ///
    /// If the bounding box is fully inside the current clip, we just emit
    /// glyph quads. If fully outside, we emit nothing. If partial, we
    /// bracket the glyphs with SetClip(clip) / SetClip(screen) so the
    /// scissor only kicks in for this draw.
    pub fn draw_text(&mut self, atlas: &Atlas, text: &BStr, x: f32, y: f32, color: [u8; 4]) {
        let clip = self.current_clip();
        let cell_w = atlas.cell_w as f32;
        let cell_h = atlas.cell_h as f32;
        let bbox = Rect {
            x,
            y,
            w: cell_w * text.chars().count() as f32,
            h: cell_h,
        };
        let state = classify_clip(bbox, clip);
        if state == ClipState::Outside {
            return;
        }
        let needs_scissor = state == ClipState::Partial;
        if needs_scissor {
            self.commands.push(DrawCommand::SetClip(clip));
        }
        let mut pen_x = x;
        for ch in text.chars() {
            // Unknown chars fall back to the tofu (same role as TTF's
            // glyph 0).
            let g = atlas.glyphs.get(&ch).unwrap_or(&atlas.missing);
            self.commands.push(DrawCommand::Quad(Quad {
                dst_x: pen_x,
                dst_y: y,
                dst_w: cell_w,
                dst_h: cell_h,
                src_x: g.atlas_x,
                src_y: g.atlas_y,
                src_w: atlas.cell_w,
                src_h: atlas.cell_h,
                color,
            }));
            pen_x += cell_w;
        }
        if needs_scissor {
            self.commands.push(DrawCommand::SetClip(self.clip_stack[0]));
        }
    }
}
