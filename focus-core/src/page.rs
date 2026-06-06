use bstr::BStr;
use std::os::unix::ffi::OsStrExt;

use crate::{
    app::{App, IO},
    document::{DocumentId, Source, SourceFile},
    drawing::{Drawing, Rect},
    editor::EditorId,
    input::InputEvent,
};

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy, Debug)]
pub struct PageId(pub(crate) usize);

pub enum Page {
    Single {
        editor_id: EditorId,
        status_bar_id: EditorId,
    },
}

impl PageId {
    pub(crate) fn get<'a>(self, app: &'a App) -> &'a Page {
        app.pages.get(&self).unwrap()
    }

    #[allow(dead_code)]
    pub(crate) fn get_mut<'a>(self, app: &'a mut App) -> &'a mut Page {
        app.pages.get_mut(&self).unwrap()
    }

    pub(crate) fn assert_invariants(self, _app: &App) {}

    pub(crate) fn document_id(self, app: &App) -> DocumentId {
        match self.get(app) {
            Page::Single { editor_id, .. } => editor_id.get(app).document_id,
        }
    }

    pub(crate) fn tick(self, app: &mut App, io: &mut dyn IO) {
        match self.get(app) {
            &Page::Single {
                editor_id,
                status_bar_id,
            } => {
                editor_id.tick(app, io);

                let status_bar_document_id = status_bar_id.get(app).document_id;
                let cursor_main_offset = editor_id.get(app).cursors.last().unwrap().head.offset;
                let grid = editor_id.grid_from_offset(app, cursor_main_offset);
                let source = match status_bar_document_id.source(app) {
                    Source::Scratch => BStr::new("scratch"),
                    Source::File(SourceFile { absolute_path, .. }) => {
                        BStr::new(absolute_path.as_os_str().as_bytes())
                    }
                };
                let status_text = format!("{} {}:{}", source, grid[0][1] + 1, grid[0][0] + 1);
                status_bar_document_id.replace(app, BStr::new(status_text.as_bytes()));

                status_bar_id.tick(app, io);
            }
        }
    }

    pub(crate) fn input(self, app: &mut App, io: &mut dyn IO, event: InputEvent<'_>) {
        match self.get(app) {
            Page::Single { editor_id, .. } => editor_id.input(app, io, event),
        }
    }

    pub(crate) fn draw(self, app: &mut App, drawing: &mut Drawing) {
        match self.get(app) {
            &Page::Single {
                editor_id,
                status_bar_id,
            } => {
                let status_bar_size = [drawing.size()[0], app.cell_size()[1] as f32];
                let mut editor_size = drawing.size();
                editor_size[1] -= status_bar_size[1];

                {
                    let mut drawing = drawing.push_clip_rect(Rect {
                        pos: [0.0, 0.0],
                        size: editor_size,
                    });
                    editor_id.draw(app, &mut drawing);
                }

                {
                    let mut drawing = drawing.push_clip_rect(Rect {
                        pos: [0.0, editor_size[1]],
                        size: status_bar_size,
                    });
                    status_bar_id.draw(app, &mut drawing);
                }
            }
        }
    }
}
