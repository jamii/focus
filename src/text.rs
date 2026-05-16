// Glyph atlas + text layout + draw-command list.
//
// The model is borrowed from rxi's microui:
//   * the "editor" side builds a Frame: a sequence of DrawCommands made of
//     Quads (colored textured rectangles) interleaved with SetClip commands
//     (axis-aligned scissor rects);
//   * the renderer (whether GPU or CPU) walks the commands in order, batching
//     consecutive Quads into a single draw call and switching scissor
//     whenever it hits a SetClip.
//
// Clipping itself is hybrid:
//   * draw_rect intersects in software — trimming a rectangle to its clip
//     is a single rect-intersection, much cheaper than emitting a scissor
//     change, and it lets the rect stay in the current batch;
//   * draw_text emits a SetClip if (and only if) the text bounding box is
//     partially outside the current clip, then a SetClip back to "no clip"
//     after the glyph quads. Per-glyph software clipping would mean
//     trimming each quad's dst rect *and* its UVs proportionally — easy to
//     get wrong, which is what happened in master.
//
// The atlas is RGBA8 with every glyph stored as (255, 255, 255, alpha) plus
// a single solid-white texel. Combined with the renderer's per-vertex color
// modulation, one shader handles tinted glyphs and flat-color rects.

use std::collections::HashMap;

use fontdue::{Font, FontSettings};

// We only handle printable ASCII for now — code points 32 (' ') through
// 126 ('~'). 95 characters total.
const ASCII_FIRST: u32 = 32;
const ASCII_LAST: u32 = 126;

// =====================================================================
// Rectangles + clip checks
// =====================================================================

#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
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

// =====================================================================
// Atlas
// =====================================================================

// Per-glyph data. Coordinate conventions throughout this module:
//   * x increases to the right, y increases downward (screen convention).
//   * "Baseline" is the imaginary line letters sit on. Descenders ('g',
//     'y') hang below it.
//   * "Pen position" is where we'd be if we hadn't drawn this glyph yet,
//     with y at the baseline.
pub struct Glyph {
    pub atlas_x: u32,
    pub atlas_y: u32,
    pub width: u32,
    pub height: u32,
    pub bearing_x: f32,
    pub bearing_y: f32,
    pub advance: f32,
}

pub struct Atlas {
    // RGBA8 texture, laid out row-major. Glyph texels are (255, 255, 255,
    // alpha); the white_rect texel is (255, 255, 255, 255).
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,

    pub glyphs: HashMap<char, Glyph>,

    // Coordinates of a single solid-white texel. Sampling here gives
    // (1, 1, 1, 1), which after the renderer's color modulation yields a
    // flat fill of the quad's per-vertex color.
    pub white_x: u32,
    pub white_y: u32,

    pub ascent: f32,
    pub line_height: f32,
}

impl Atlas {
    pub fn build(font_bytes: &[u8], px_size: f32) -> Self {
        let font = Font::from_bytes(font_bytes, FontSettings::default()).unwrap();
        let line = font.horizontal_line_metrics(px_size).unwrap();

        // First pass: rasterize everything, find max cell dimensions.
        let mut raster: Vec<(char, fontdue::Metrics, Vec<u8>)> = Vec::new();
        let mut max_w = 1u32;
        let mut max_h = 1u32;
        for cp in ASCII_FIRST..=ASCII_LAST {
            let ch = char::from_u32(cp).unwrap();
            let (m, bitmap) = font.rasterize(ch, px_size);
            max_w = max_w.max(m.width as u32);
            max_h = max_h.max(m.height as u32);
            raster.push((ch, m, bitmap));
        }

        // 16 × 6 cells holds 95 glyphs plus a spare for the white texel.
        let cols = 16u32;
        let rows = raster.len().div_ceil(cols as usize) as u32;
        let cell_w = max_w;
        let cell_h = max_h;
        let atlas_w = cols * cell_w;
        let atlas_h = rows * cell_h;

        let mut pixels = vec![0u8; (atlas_w * atlas_h * 4) as usize];
        let mut glyphs = HashMap::new();
        for (i, (ch, m, bitmap)) in raster.iter().enumerate() {
            let col = i as u32 % cols;
            let row = i as u32 / cols;
            let ax = col * cell_w;
            let ay = row * cell_h;
            let gw = m.width as u32;
            let gh = m.height as u32;

            for j in 0..gh {
                for k in 0..gw {
                    let src_idx = (j * gw + k) as usize;
                    let dst_idx = (((ay + j) * atlas_w + (ax + k)) * 4) as usize;
                    pixels[dst_idx] = 255;
                    pixels[dst_idx + 1] = 255;
                    pixels[dst_idx + 2] = 255;
                    pixels[dst_idx + 3] = bitmap[src_idx];
                }
            }

            // fontdue uses font-coordinate y-up: ymin is the y of the
            // bottom-most pixel relative to the baseline (negative for
            // descenders). bearing_y = top of bitmap above baseline.
            glyphs.insert(
                *ch,
                Glyph {
                    atlas_x: ax,
                    atlas_y: ay,
                    width: gw,
                    height: gh,
                    bearing_x: m.xmin as f32,
                    bearing_y: (m.ymin + m.height as i32) as f32,
                    advance: m.advance_width,
                },
            );
        }

        // White texel in the spare bottom-right cell.
        let white_x = atlas_w - 1;
        let white_y = atlas_h - 1;
        let white_idx = ((white_y * atlas_w + white_x) * 4) as usize;
        pixels[white_idx] = 255;
        pixels[white_idx + 1] = 255;
        pixels[white_idx + 2] = 255;
        pixels[white_idx + 3] = 255;

        Atlas {
            width: atlas_w,
            height: atlas_h,
            pixels,
            glyphs,
            white_x,
            white_y,
            ascent: line.ascent,
            line_height: line.new_line_size,
        }
    }

    fn measure_text(&self, text: &str) -> f32 {
        text.chars()
            .map(|c| self.glyphs.get(&c).map(|g| g.advance).unwrap_or(0.0))
            .sum()
    }
}

// =====================================================================
// Quad and primitive emitters
// =====================================================================

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

// Walk a string and produce one Quad per visible character, all tinted
// with `color`. (x, y) is the top-left of the line's bounding box;
// internally we move the pen along the baseline at y + atlas.ascent.
// Characters whose bitmap is empty (notably space) emit no quad but do
// advance the pen.
pub fn layout_text(atlas: &Atlas, text: &str, x: f32, y: f32, color: [u8; 4]) -> Vec<Quad> {
    let baseline = y + atlas.ascent;
    let mut pen_x = x;
    let mut quads = Vec::new();
    for ch in text.chars() {
        let Some(g) = atlas.glyphs.get(&ch) else {
            continue;
        };
        if g.width > 0 && g.height > 0 {
            quads.push(Quad {
                dst_x: pen_x + g.bearing_x,
                dst_y: baseline - g.bearing_y,
                dst_w: g.width as f32,
                dst_h: g.height as f32,
                src_x: g.atlas_x,
                src_y: g.atlas_y,
                src_w: g.width,
                src_h: g.height,
                color,
            });
        }
        pen_x += g.advance;
    }
    quads
}

// Flat-colored rectangle using the atlas's white_rect. The GPU samples
// solid white across the whole quad and the per-vertex color comes through
// unchanged.
pub fn solid_rect(atlas: &Atlas, x: f32, y: f32, w: f32, h: f32, color: [u8; 4]) -> Quad {
    Quad {
        dst_x: x,
        dst_y: y,
        dst_w: w,
        dst_h: h,
        src_x: atlas.white_x,
        src_y: atlas.white_y,
        src_w: 1,
        src_h: 1,
        color,
    }
}

// =====================================================================
// Draw-command list (microui-style)
// =====================================================================

#[derive(Clone, Debug)]
pub enum DrawCommand {
    Quad(Quad),
    SetClip(Rect),
}

// A frame's worth of drawing, built up by the "editor" and consumed by the
// renderer. Maintains a clip-rect stack; each push intersects with the
// current top, so pushing only ever shrinks the clip.
pub struct Frame {
    commands: Vec<DrawCommand>,
    clip_stack: Vec<Rect>,
    // The sentinel "no clip" rect — the renderer's GL_SCISSOR_TEST is
    // always on, so we set the scissor to this rect to mean "draw
    // everywhere". Same idea as microui's `unclipped_rect`.
    unclipped: Rect,
}

impl Frame {
    pub fn new(screen_w: f32, screen_h: f32) -> Self {
        let unclipped = Rect::new(0.0, 0.0, screen_w, screen_h);
        Self {
            commands: Vec::new(),
            clip_stack: vec![unclipped],
            unclipped,
        }
    }

    pub fn commands(&self) -> &[DrawCommand] {
        &self.commands
    }

    pub fn current_clip(&self) -> Rect {
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
            self.commands.push(DrawCommand::Quad(solid_rect(
                atlas, trimmed.x, trimmed.y, trimmed.w, trimmed.h, color,
            )));
        }
    }

    /// Text, clipped via GL scissor if necessary.
    ///
    /// If the text's bounding box is fully inside the current clip, we
    /// just emit the glyph quads. If it's fully outside, we emit nothing.
    /// If it's partial, we bracket the glyph quads with SetClip commands:
    /// scissor to the clip rect for the duration of the text, then scissor
    /// back to "unclipped" so following draws don't pay the cost.
    pub fn draw_text(&mut self, atlas: &Atlas, text: &str, x: f32, y: f32, color: [u8; 4]) {
        let bbox = Rect::new(x, y, atlas.measure_text(text), atlas.line_height);
        let clip = self.current_clip();
        match classify_clip(bbox, clip) {
            ClipState::Outside => return,
            ClipState::Inside => {
                for q in layout_text(atlas, text, x, y, color) {
                    self.commands.push(DrawCommand::Quad(q));
                }
            }
            ClipState::Partial => {
                self.commands.push(DrawCommand::SetClip(clip));
                for q in layout_text(atlas, text, x, y, color) {
                    self.commands.push(DrawCommand::Quad(q));
                }
                self.commands.push(DrawCommand::SetClip(self.unclipped));
            }
        }
    }
}

// =====================================================================
// CPU rasterizer (test harness)
// =====================================================================

// Walk a command list and produce an RGBA8 buffer. Mirrors what the GPU
// renderer does: GL_SCISSOR_TEST is conceptually always on; SetClip
// updates the current scissor; Quads are rasterized, with any pixels
// outside the scissor discarded.
pub fn composite(atlas: &Atlas, commands: &[DrawCommand], width: u32, height: u32) -> Vec<u8> {
    let mut buf = vec![255u8; (width * height * 4) as usize];
    let mut scissor = Rect::new(0.0, 0.0, width as f32, height as f32);

    for cmd in commands {
        match cmd {
            DrawCommand::SetClip(r) => scissor = *r,
            DrawCommand::Quad(q) => rasterize_quad(atlas, q, scissor, &mut buf, width, height),
        }
    }
    buf
}

fn rasterize_quad(atlas: &Atlas, q: &Quad, scissor: Rect, buf: &mut [u8], width: u32, height: u32) {
    let dx0 = q.dst_x.round() as i32;
    let dy0 = q.dst_y.round() as i32;
    let dw = q.dst_w.round() as i32;
    let dh = q.dst_h.round() as i32;
    if dw <= 0 || dh <= 0 {
        return;
    }

    let sx0 = scissor.x.floor() as i32;
    let sy0 = scissor.y.floor() as i32;
    let sx1 = (scissor.x + scissor.w).ceil() as i32;
    let sy1 = (scissor.y + scissor.h).ceil() as i32;

    for j in 0..dh {
        let dy = dy0 + j;
        if dy < sy0 || dy >= sy1 || dy < 0 || dy >= height as i32 {
            continue;
        }
        for i in 0..dw {
            let dx = dx0 + i;
            if dx < sx0 || dx >= sx1 || dx < 0 || dx >= width as i32 {
                continue;
            }

            // Map destination (i, j) → source texel.
            let si = (i * q.src_w as i32 / dw).min(q.src_w as i32 - 1);
            let sj = (j * q.src_h as i32 / dh).min(q.src_h as i32 - 1);
            let src_idx =
                (((q.src_y as i32 + sj) * atlas.width as i32 + (q.src_x as i32 + si)) * 4) as usize;

            let ta = atlas.pixels[src_idx + 3] as f32 / 255.0;
            let cr = q.color[0] as f32;
            let cg = q.color[1] as f32;
            let cb = q.color[2] as f32;
            let ca = q.color[3] as f32 / 255.0;
            let sa = ca * ta;

            let idx = ((dy as u32 * width + dx as u32) * 4) as usize;
            let dr = buf[idx] as f32;
            let dg = buf[idx + 1] as f32;
            let db = buf[idx + 2] as f32;

            buf[idx] = (cr * sa + dr * (1.0 - sa)) as u8;
            buf[idx + 1] = (cg * sa + dg * (1.0 - sa)) as u8;
            buf[idx + 2] = (cb * sa + db * (1.0 - sa)) as u8;
            buf[idx + 3] = 255;
        }
    }
}
