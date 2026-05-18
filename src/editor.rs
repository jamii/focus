use bstr::{BStr, ByteSlice};
use winit::{
    event::ElementState,
    keyboard::{Key, NamedKey},
};

use crate::{
    app::{App, DocumentId, IO, InputEvent},
    document::{Edit, EditKind},
    style::{HIGHLIGHT_COLOR, TEXT_COLOR},
    text::{Drawing, Rect},
};

pub struct Editor {
    pub document_id: DocumentId,
    cursors: Vec<Cursor>,
    show_cursor: bool,
    wrap_chars: usize,
    wraps: Vec<[usize; 2]>,
}

struct Cursor {
    pos: usize,
}

impl Editor {
    pub fn new(document_id: DocumentId) -> Self {
        Editor {
            document_id: document_id,
            cursors: vec![Cursor { pos: 0 }],
            show_cursor: true,
            wrap_chars: 0,
            wraps: vec![[0, 0]],
        }
    }

    pub fn input(&mut self, app: &App, _io: &mut dyn IO, event: InputEvent) {
        let mut document = self.document_id.get_mut(app);
        match event {
            InputEvent::KeyboardInput {
                event: key_event, ..
            } if key_event.state == ElementState::Pressed
                && !app.modifiers.state().control_key()
                && !app.modifiers.state().alt_key() =>
            {
                match key_event.logical_key.as_ref() {
                    Key::Character(char) => {
                        document.queue_edits(
                            self.cursors
                                .iter()
                                .map(|c| Edit {
                                    kind: EditKind::Insert,
                                    pos: c.pos,
                                    text: char.into(),
                                })
                                .collect(),
                        );
                    }
                    Key::Named(NamedKey::Enter) => {
                        document.queue_edits(
                            self.cursors
                                .iter()
                                .map(|c| Edit {
                                    kind: EditKind::Insert,
                                    pos: c.pos,
                                    text: '\n'.to_string().into(),
                                })
                                .collect(),
                        );
                    }
                    Key::Named(NamedKey::Space) => {
                        document.queue_edits(
                            self.cursors
                                .iter()
                                .map(|c| Edit {
                                    kind: EditKind::Insert,
                                    pos: c.pos,
                                    text: ' '.to_string().into(),
                                })
                                .collect(),
                        );
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    pub fn tick(&mut self, _app: &App, io: &mut dyn IO, redraw: &mut bool) {
        let show_cursor = ((io.frame_start().as_millis() / 500) % 2) == 0;
        if self.show_cursor != show_cursor {
            self.show_cursor = show_cursor;
            *redraw = true;
        }
    }

    pub fn draw(&mut self, app: &App, drawing: &mut Drawing) {
        let wrap_chars = (drawing.current_clip_size()[0] / app.atlas.cell_size[0] as f32).floor() as isize
            // leave space for gutters
            - 2;
        if wrap_chars <= 0 {
            return;
        }
        if self.wrap_chars != wrap_chars as usize {
            self.wrap_chars = wrap_chars as usize;
            self.refresh_wraps(app);
        }

        {
            let text = &self.document_id.get(app).text;
            for [start, end] in &self.wraps {
                let mut grid = self.grid_from_pos(app, *start);
                grid[0] += 1; // gutter
                drawing.draw_text(
                    &app.atlas,
                    &text.as_bstr()[*start..*end],
                    app.atlas.screen_from_grid(grid),
                    TEXT_COLOR,
                );
                if *start > 0 && text[start - 1] != b'\n' {
                    drawing.draw_text(
                        &app.atlas,
                        BStr::new(b"\\"),
                        app.atlas.screen_from_grid([0, grid[1]]),
                        HIGHLIGHT_COLOR,
                    );
                }
            }
        }

        if self.show_cursor {
            for cursor in &self.cursors {
                let mut grid = self.grid_from_pos(app, cursor.pos);
                grid[0] += 1; // gutter
                let mut pos = app.atlas.screen_from_grid(grid);
                let mut size = [app.atlas.cell_size[0] as f32, app.atlas.cell_size[1] as f32];
                size[0] /= 8.0;
                pos[0] -= size[0] / 2.0;
                drawing.draw_rect(&app.atlas, Rect { pos, size }, TEXT_COLOR);
            }
        }
    }

    pub fn handle_edits(&mut self, app: &App, edits: &[Edit]) {
        for cursor in &mut self.cursors {
            let mut pos_diff = 0;
            for edit in edits.iter() {
                if cursor.pos < edit.pos {
                    break;
                }
                match edit.kind {
                    EditKind::Insert => {
                        pos_diff += edit.text.len();
                    }
                    EditKind::Delete => {
                        pos_diff -= edit.text.len();
                    }
                }
            }
            cursor.pos += pos_diff;
        }
        self.refresh_wraps(app);
    }

    fn refresh_wraps(&mut self, app: &App) {
        self.wraps.clear();
        let text = &self.document_id.get(app).text;
        let mut start = 0;
        let mut end = 0;
        let mut last_soft_wrap = None;
        let mut col = 0;
        while let Some((_, char_end, char)) = text[end..].char_indices().next() {
            if col >= self.wrap_chars {
                if let Some(pos) = last_soft_wrap {
                    end = pos;
                }
                self.wraps.push([start, end]);
                start = end;
                col = 0;
                last_soft_wrap = None;
            }
            if char == '\n' {
                self.wraps.push([start, end]);
                start = end + char_end;
                col = 0;
                last_soft_wrap = None;
            } else {
                col += 1
            }
            end += char_end;
            if char == ' ' {
                last_soft_wrap = Some(end);
            }
        }
        self.wraps.push([start, text.len()]);
    }

    fn grid_from_pos(&self, app: &App, pos: usize) -> [usize; 2] {
        // TODO binary search
        let text = &self.document_id.get(app).text;
        for (line, [start, end]) in self.wraps.iter().enumerate().rev() {
            if *start <= pos && pos <= *end {
                let col = text[*start..pos].chars().count();
                return [col, line];
            }
        }
        unreachable!();
    }
}
