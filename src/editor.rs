use crate::{
    app::{App, DocumentId},
    text::Drawing,
};

pub struct Editor {
    pub document_id: DocumentId,
}
impl Editor {
    pub fn new(document_id: DocumentId) -> Self {
        Editor {
            document_id: document_id,
        }
    }

    pub fn draw(&self, app: &App, drawing: &mut Drawing) {
        app.get_document(self.document_id).draw(app, drawing);
    }
}
