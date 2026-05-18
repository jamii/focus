use bstr::{BString, ByteSlice};

use crate::style::TEXT_COLOR;
use crate::{app::App, text::Drawing};

pub struct Document {
    pub text: BString,
}

impl Document {
    pub fn new() -> Document {
        Document { text: "".into() }
    }

    pub fn replace(&mut self, text: BString) {
        self.text = text;
    }

    pub fn draw(&self, app: &App, drawing: &mut Drawing) {
        drawing.draw_text(&app.atlas, self.text.as_bstr(), 0.0, 0.0, TEXT_COLOR);
    }
}
