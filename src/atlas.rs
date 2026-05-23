// Glyph atlas — rasterizes printable ASCII into a grid of cells.
//
// The atlas is R8 (single-channel alpha) with every glyph stored as
// coverage alpha ∈ [0, 255]. Combined with the renderer's per-vertex
// color modulation, one shader handles tinted glyphs and flat-color rects
// (the latter sampling a single solid-white texel).

use std::collections::HashMap;

use fontdue::Font;

// We only handle printable ASCII for now — code points 32 (' ') through
// 126 ('~'). 95 characters total.
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
    // ∈ [0, 255]; the fragment shader supplies the color. The white_pos
    // texel is just 255 (= "fully opaque").
    pub size: [u32; 2],
    pub pixels: Vec<u8>,

    pub glyphs: HashMap<char, Glyph>,

    // Glyph 0 (.notdef) from the font.
    pub notdef: Glyph,

    // Coordinates of a single solid-white texel. Lives in its own
    // dedicated cell that no glyph quad ever samples; draw_rect points
    // at it for flat-colored fills.
    pub white_pos: [u32; 2],

    // Cell dimensions, in texels = screen pixels (we draw 1:1).
    // cell_size[0] is the rounded-up advance width; cell_size[1] is the
    // rounded-up line_height so a single cell fits any glyph from ascent
    // to descent.
    pub cell_size: [u32; 2],
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

        // Rasterize glyph 0 (the font's .notdef).
        let (notdef_m, notdef_bitmap) = font.rasterize_indexed(0, px_size);
        max_advance = max_advance.max(notdef_m.advance_width);

        let cell_w = max_advance.ceil() as u32;
        let cell_h = line.new_line_size.ceil() as u32;

        // Layout: ASCII glyphs, then notdef, then white texel. Each lives
        // in its own cell. 95 + 1 + 1 = 97 cells = 7 rows × 16 cols with
        // 15 unused cells at the end.
        let notdef_idx = NUM_GLYPHS as u32;
        let white_idx = notdef_idx + 1;
        let rows = (white_idx + 1).div_ceil(ATLAS_COLS);
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
                    atlas_pos: [cell_x, cell_y],
                },
            );
        }

        // Paint glyph 0 (.notdef) into the missing-glyph cell.
        let [tx, ty] = cell_origin(notdef_idx);
        let bitmap_left = notdef_m.xmin;
        let bitmap_top = ascent as i32 - (notdef_m.ymin + notdef_m.height as i32);
        let gw = notdef_m.width as i32;
        let gh = notdef_m.height as i32;
        for j in 0..gh {
            for k in 0..gw {
                let cx = bitmap_left + k;
                let cy = bitmap_top + j;
                if cx < 0 || cy < 0 || cx >= cell_w as i32 || cy >= cell_h as i32 {
                    continue;
                }
                let dst = ((ty + cy as u32) * atlas_w + (tx + cx as u32)) as usize;
                pixels[dst] = notdef_bitmap[(j * gw + k) as usize];
            }
        }
        let notdef = Glyph {
            atlas_pos: [tx, ty],
        };

        // White texel in its own cell.
        let white_pos = cell_origin(white_idx);
        pixels[(white_pos[1] * atlas_w + white_pos[0]) as usize] = 255;

        Atlas {
            size: [atlas_w, atlas_h],
            pixels,
            glyphs,
            notdef,
            white_pos,
            cell_size: [cell_w, cell_h],
        }
    }

    /// Top-left screen position of the cell at the given grid coords.
    pub fn screen_from_grid(&self, grid: [usize; 2]) -> [f32; 2] {
        [
            (grid[0] as f32) * (self.cell_size[0] as f32),
            (grid[1] as f32) * (self.cell_size[1] as f32),
        ]
    }

    /// Grid cell containing the given screen position. Floor-divides, so
    /// a screen position on a cell boundary lands in the cell to its
    /// right / below.
    pub fn grid_from_screen(&self, screen: [f32; 2]) -> [i32; 2] {
        [
            (screen[0] / self.cell_size[0] as f32).floor() as i32,
            (screen[1] / self.cell_size[1] as f32).floor() as i32,
        ]
    }
}
