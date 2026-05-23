// Draw-command list for a single frame.
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

use bstr::{BStr, ByteSlice};

use crate::atlas::Atlas;

#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub pos: [f32; 2],
    pub size: [f32; 2],
}

impl Rect {
    pub fn from_corners(start: [f32; 2], end: [f32; 2]) -> Self {
        Rect {
            pos: start,
            size: [end[0] - start[0], end[1] - start[1]],
        }
    }
}

// Axis-aligned intersection. Returns a rect with non-positive size when the
// inputs don't overlap; callers check size[0] > 0 && size[1] > 0 before drawing.
pub fn intersect_rects(a: Rect, b: Rect) -> Rect {
    let x0 = a.pos[0].max(b.pos[0]);
    let y0 = a.pos[1].max(b.pos[1]);
    let x1 = (a.pos[0] + a.size[0]).min(b.pos[0] + b.size[0]);
    let y1 = (a.pos[1] + a.size[1]).min(b.pos[1] + b.size[1]);
    Rect {
        pos: [x0, y0],
        size: [(x1 - x0).max(0.0), (y1 - y0).max(0.0)],
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
    if rect.pos[0] >= clip.pos[0] + clip.size[0]
        || rect.pos[0] + rect.size[0] <= clip.pos[0]
        || rect.pos[1] >= clip.pos[1] + clip.size[1]
        || rect.pos[1] + rect.size[1] <= clip.pos[1]
    {
        return ClipState::Outside;
    }
    if rect.pos[0] >= clip.pos[0]
        && rect.pos[0] + rect.size[0] <= clip.pos[0] + clip.size[0]
        && rect.pos[1] >= clip.pos[1]
        && rect.pos[1] + rect.size[1] <= clip.pos[1] + clip.size[1]
    {
        return ClipState::Inside;
    }
    ClipState::Partial
}

// A rectangle to draw: `dst_*` is screen-space (in pixels), `src_*` is
// atlas-space (in texels), `color` is the per-vertex color modulated
// into the sampled texel.
//
//   text glyph  → src is a glyph cell,    color is text color
//   solid fill  → src is the 1×1 white,   color is fill color
#[derive(Clone, Copy, Debug)]
pub struct Quad {
    pub dst_pos: [f32; 2],
    pub dst_size: [f32; 2],
    pub src_pos: [u32; 2],
    pub src_size: [u32; 2],
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
    pub fn new(screen_size: [f32; 2]) -> Self {
        Self {
            commands: Vec::new(),
            clip_stack: vec![Rect {
                pos: [0.0, 0.0],
                size: screen_size,
            }],
        }
    }

    pub fn commands(&self) -> &[DrawCommand] {
        &self.commands
    }

    fn current_clip(&self) -> Rect {
        *self.clip_stack.last().unwrap()
    }

    pub fn size(&self) -> [f32; 2] {
        self.current_clip().size
    }

    /// Push a new clip, intersected with the current top of the stack.
    /// Subsequent draws will be clipped to this rectangle. The returned
    /// `ClipScope` pops the clip when it is dropped.
    pub fn push_clip_rect(&mut self, rect: Rect) -> ClipScope<'_> {
        let top = self.current_clip();
        self.clip_stack.push(intersect_rects(rect, top));
        ClipScope { drawing: self }
    }

    /// Solid rectangle. Trimmed against the current clip in software, so
    /// it doesn't need to break the batch.
    pub fn draw_rect(&mut self, atlas: &Atlas, rect: Rect, color: [u8; 4]) {
        let clip = self.current_clip();
        let abs_rect = Rect {
            pos: [clip.pos[0] + rect.pos[0], clip.pos[1] + rect.pos[1]],
            size: rect.size,
        };
        let trimmed = intersect_rects(abs_rect, clip);
        if trimmed.size[0] > 0.0 && trimmed.size[1] > 0.0 {
            self.commands.push(DrawCommand::Quad(Quad {
                dst_pos: trimmed.pos,
                dst_size: trimmed.size,
                src_pos: atlas.white_pos,
                src_size: [1, 1],
                color,
            }));
        }
    }

    /// Text, clipped via GL scissor if necessary. `pos` is the top-left
    /// of the line's bounding box.
    ///
    /// If the bounding box is fully inside the current clip, we just emit
    /// glyph quads. If fully outside, we emit nothing. If partial, we
    /// bracket the glyphs with SetClip(clip) / SetClip(screen) so the
    /// scissor only kicks in for this draw.
    pub fn draw_text(&mut self, atlas: &Atlas, text: &BStr, pos: [f32; 2], color: [u8; 4]) {
        let clip = self.current_clip();
        let abs_pos = [clip.pos[0] + pos[0], clip.pos[1] + pos[1]];
        let cell_w = atlas.cell_size[0] as f32;
        let cell_h = atlas.cell_size[1] as f32;
        let bbox = Rect {
            pos: abs_pos,
            size: [cell_w * text.chars().count() as f32, cell_h],
        };
        let state = classify_clip(bbox, clip);
        if state == ClipState::Outside {
            return;
        }
        let needs_scissor = state == ClipState::Partial;
        if needs_scissor {
            self.commands.push(DrawCommand::SetClip(clip));
        }
        let mut pen_x = abs_pos[0];
        for char in text.chars() {
            // Unknown chars fall back to the tofu (same role as TTF's
            // glyph 0).
            let char = match char {
                '\n' => ' ',
                _ => char,
            };
            let g = atlas.glyphs.get(&char).unwrap_or(&atlas.notdef);
            self.commands.push(DrawCommand::Quad(Quad {
                dst_pos: [pen_x, abs_pos[1]],
                dst_size: [cell_w, cell_h],
                src_pos: g.atlas_pos,
                src_size: atlas.cell_size,
                color,
            }));
            pen_x += cell_w;
        }
        if needs_scissor {
            self.commands.push(DrawCommand::SetClip(self.clip_stack[0]));
        }
    }
}

/// RAII guard returned by `Drawing::push_clip_rect`. Derefs to the
/// underlying `Drawing`, so callers can draw through it; on drop, it pops
/// the clip it pushed.
pub struct ClipScope<'a> {
    drawing: &'a mut Drawing,
}

impl<'a> std::ops::Deref for ClipScope<'a> {
    type Target = Drawing;
    fn deref(&self) -> &Drawing {
        self.drawing
    }
}

impl<'a> std::ops::DerefMut for ClipScope<'a> {
    fn deref_mut(&mut self) -> &mut Drawing {
        self.drawing
    }
}

impl<'a> Drop for ClipScope<'a> {
    fn drop(&mut self) {
        assert!(
            self.drawing.clip_stack.len() > 1,
            "popped the initial (screen) clip"
        );
        self.drawing.clip_stack.pop();
    }
}
