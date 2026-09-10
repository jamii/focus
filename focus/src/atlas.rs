// Glyph atlas — rasterizes printable ASCII into a grid of cells.
//
// The atlas is R8 (single-channel alpha) with every glyph stored as
// coverage alpha ∈ [0, 255]. Combined with the renderer's per-vertex
// color modulation, one shader handles tinted glyphs and flat-color rects
// (the latter drawn as the Full Block '█', a cell painted fully opaque).
//
// Lives in the `focus` crate, not the core: the core only ever deals in
// characters and the cell size. Turning a character into pixels — which
// font, which atlas cell — is the renderer's job.

use std::collections::HashMap;

use fontdue::Font;

use focus_core::drawing::FULL_BLOCK;

// We only handle printable ASCII for now — code points 32 (' ') through
// 126 ('~'). 95 characters total. Plus the Full Block '█', synthesized as
// a fully-opaque cell so solid fills are exactly solid.
const ASCII_FIRST: u32 = 32;
const ASCII_LAST: u32 = 126;
const NUM_GLYPHS: usize = (ASCII_LAST - ASCII_FIRST + 1) as usize;
const ATLAS_COLS: u32 = 16;

// A glyph is just the top-left of its cell in the atlas. Each cell is
// `cell_size[0] × cell_size[1]` texels, with the glyph pre-positioned at
// its baseline-relative offset within that cell — so drawing a character
// is just "blit the whole cell at (pen_x, y)". No per-glyph bearings.
#[derive(Clone, Copy)]
pub struct Glyph {
    pub atlas_pos: [u32; 2],
}

pub struct Atlas {
    // R8 texture, laid out row-major. Each texel stores coverage alpha
    // ∈ [0, 255]; the fragment shader supplies the color.
    pub size: [u32; 2],
    pub pixels: Vec<u8>,

    pub glyphs: HashMap<char, Glyph>,

    // Glyph 0 (.notdef) from the font. Used for any character we didn't
    // rasterize.
    pub notdef: Glyph,

    // Cell dimensions, in texels = screen pixels (we draw 1:1).
    // cell_size[0] is the rounded-up advance width; cell_size[1] is the
    // rounded-up line_height so a single cell fits any glyph from ascent
    // to descent.
    pub cell_size: [u32; 2],
}

impl Atlas {
    pub fn build(font: &Font, font_size: f32) -> Self {
        let line = font.horizontal_line_metrics(font_size).unwrap();
        let ascent = line.ascent;

        // Rasterize all printable ASCII; collect bitmaps + metrics and
        // find the constant advance width.
        let mut raster: Vec<(char, fontdue::Metrics, Vec<u8>)> = Vec::with_capacity(NUM_GLYPHS);
        let mut max_advance = 0.0_f32;
        for cp in ASCII_FIRST..=ASCII_LAST {
            let ch = char::from_u32(cp).unwrap();
            let (m, bitmap) = font.rasterize(ch, font_size);
            max_advance = max_advance.max(m.advance_width);
            raster.push((ch, m, bitmap));
        }

        // Rasterize glyph 0 (the font's .notdef).
        let (notdef_m, notdef_bitmap) = font.rasterize_indexed(0, font_size);
        max_advance = max_advance.max(notdef_m.advance_width);

        let cell_w = max_advance.ceil() as u32;
        let cell_h = line.new_line_size.ceil() as u32;

        // Layout: ASCII glyphs, then notdef, then the Full Block cell. Each
        // lives in its own cell. 95 + 1 + 1 = 97 cells = 7 rows × 16 cols
        // with 15 unused cells at the end.
        let notdef_idx = NUM_GLYPHS as u32;
        let block_idx = notdef_idx + 1;
        let rows = (block_idx + 1).div_ceil(ATLAS_COLS);
        let atlas_w = ATLAS_COLS * cell_w;
        let atlas_h = rows * cell_h;
        let mut pixels = vec![0u8; (atlas_w * atlas_h) as usize];

        let cell_origin = |idx: u32| [(idx % ATLAS_COLS) * cell_w, (idx / ATLAS_COLS) * cell_h];

        // For each glyph, paint its bitmap into its cell at the
        // bearing-offset that puts it on the cell's baseline. The cell's
        // baseline is at y = ascent from the top:
        //   bitmap_top_in_cell  = ascent - (ymin + height)
        //                       = ascent - distance from baseline to top of bitmap
        //   bitmap_left_in_cell = xmin (fontdue's left side bearing)
        let mut glyphs = HashMap::with_capacity(NUM_GLYPHS);
        for (i, (ch, m, bitmap)) in raster.iter().enumerate() {
            let [cell_x, cell_y] = cell_origin(i as u32);
            blit(
                &mut pixels,
                atlas_w,
                cell_x,
                cell_y,
                cell_w,
                cell_h,
                ascent,
                m,
                bitmap,
            );
            glyphs.insert(
                *ch,
                Glyph {
                    atlas_pos: [cell_x, cell_y],
                },
            );
        }

        // Paint glyph 0 (.notdef) into the missing-glyph cell.
        let [tx, ty] = cell_origin(notdef_idx);
        blit(
            &mut pixels,
            atlas_w,
            tx,
            ty,
            cell_w,
            cell_h,
            ascent,
            &notdef_m,
            &notdef_bitmap,
        );
        let notdef = Glyph {
            atlas_pos: [tx, ty],
        };

        // Full Block: a cell painted fully opaque, so flat-color fills are
        // exactly solid regardless of the font.
        let [bx, by] = cell_origin(block_idx);
        for j in 0..cell_h {
            for k in 0..cell_w {
                pixels[((by + j) * atlas_w + (bx + k)) as usize] = 255;
            }
        }
        glyphs.insert(
            FULL_BLOCK,
            Glyph {
                atlas_pos: [bx, by],
            },
        );

        Atlas {
            size: [atlas_w, atlas_h],
            pixels,
            glyphs,
            notdef,
            cell_size: [cell_w, cell_h],
        }
    }

    /// The glyph for `ch`, falling back to the font's .notdef (tofu) for
    /// any character we didn't rasterize.
    pub fn glyph(&self, ch: char) -> Glyph {
        self.glyphs.get(&ch).copied().unwrap_or(self.notdef)
    }
}

// Paint a rasterized bitmap into one cell at its baseline-relative offset.
// Glyphs that overhang their cell are clamped (not expected for printable
// ASCII, but defensive).
#[allow(clippy::too_many_arguments)]
fn blit(
    pixels: &mut [u8],
    atlas_w: u32,
    cell_x: u32,
    cell_y: u32,
    cell_w: u32,
    cell_h: u32,
    ascent: f32,
    m: &fontdue::Metrics,
    bitmap: &[u8],
) {
    let bitmap_left = m.xmin;
    let bitmap_top = ascent as i32 - (m.ymin + m.height as i32);
    let gw = m.width as i32;
    let gh = m.height as i32;
    for j in 0..gh {
        for k in 0..gw {
            let cx = bitmap_left + k;
            let cy = bitmap_top + j;
            if cx < 0 || cy < 0 || cx >= cell_w as i32 || cy >= cell_h as i32 {
                continue;
            }
            let dst = ((cell_y + cy as u32) * atlas_w + (cell_x + cx as u32)) as usize;
            pixels[dst] = bitmap[(j * gw + k) as usize];
        }
    }
}
