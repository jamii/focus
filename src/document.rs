use std::time::SystemTime;
use std::{path::PathBuf, time::Duration};

use bstr::{BStr, BString, ByteSlice};

use crate::app::{App, DocumentId, IO};
use crate::editor::Editor;

pub struct Document {
    pub text: BString,
    pub newlines: Vec<usize>,
    pub last_modified_time: Duration,
    pub last_center_offset: usize,
    pub source: Source,
}

pub enum Source {
    Scratch,
    File(SourceFile),
}

pub struct SourceFile {
    absolute_path: PathBuf,
    last_load_mtime: SystemTime,
    last_save_time: Duration,
    deleted_since_last_save: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveKind {
    Explicit,
    Auto,
}

#[derive(Debug)]
pub struct Edit {
    pub kind: EditKind,
    pub offset: usize,
    pub text: BString,
}

#[derive(Debug)]
pub enum EditKind {
    Insert,
    Delete,
}

pub struct OffsetDiff {
    offsets_old: Vec<usize>, // boundaries in old-document space
    offsets_new: Vec<usize>, // new offset at each segment start (len = offsets_old.len() + 1)
    deleted: Vec<bool>, // true → clamp to offsets_new[i]; false → shift by offsets_new[i] - old_start
}

impl Document {
    pub(crate) fn scratch() -> Document {
        Document {
            text: "".into(),
            newlines: vec![],
            last_modified_time: Duration::ZERO,
            last_center_offset: 0,
            source: Source::Scratch,
        }
    }

    pub(crate) fn from_file(absolute_path: PathBuf) -> Document {
        Document {
            text: "".into(),
            newlines: vec![],
            last_modified_time: Duration::ZERO,
            last_center_offset: 0,
            source: Source::File(SourceFile {
                absolute_path,
                last_load_mtime: SystemTime::UNIX_EPOCH,
                last_save_time: Duration::ZERO,
                deleted_since_last_save: false,
            }),
        }
    }

    pub fn tick(document_id: DocumentId, app: &App, io: &mut dyn IO) {
        let mut document = document_id.get_mut(app);
        let last_modified_time = document.last_modified_time;
        let mut text_new = None;
        if let Source::File(source) = &mut document.source {
            if !(last_modified_time > source.last_save_time) {
                if let Some(text) = source.refresh_from_disk(io) {
                    source.last_save_time = io.frame_start();
                    text_new = Some(text);
                }
            }
        }
        if let Some(text_new) = text_new {
            let edits = diff_text(document.text.as_bstr(), text_new.as_bstr());
            drop(document);
            Document::apply_edits(document_id, app, edits);
        }
    }

    pub fn apply_edits(document_id: DocumentId, app: &App, edits: Vec<Edit>) {
        let mut document = document_id.get_mut(app);
        Edit::assert_invariants(&edits, document.text.as_bstr());

        let len_old = document.text.len();

        // TODO This can be made way more efficient, so that common cases don't have to allocate a whole new text.
        let mut text_new = BString::new(Vec::with_capacity(document.text.len()));
        let mut offset = 0;
        for edit in &edits {
            text_new.extend_from_slice(&document.text[offset..edit.offset]);
            offset = edit.offset;
            match edit.kind {
                EditKind::Insert => {
                    text_new.extend_from_slice(&edit.text);
                }
                EditKind::Delete => {
                    offset += edit.text.len();
                }
            }
        }
        text_new.extend_from_slice(&document.text[offset..]);
        document.text = text_new;

        // TODO This can be made way more efficient, so that common cases don't have to iterate over the whole text.
        let mut newlines = Vec::new();
        for (char_start, _, char) in document.text.char_indices() {
            if char == '\n' {
                newlines.push(char_start);
            }
        }
        document.newlines = newlines;

        let diff = OffsetDiff::from_edits(&edits, len_old);

        drop(document);

        for (editor_id, editor) in &app.editors {
            let matches = editor.borrow().document_id == document_id;
            if matches {
                Editor::handle_edits(*editor_id, app, &diff);
            }
        }
    }

    /// Save to disk. Explicit saves create the file if missing; autosaves
    /// treat NotFound as an external deletion.
    /// No-op for Scratch sources or when not modified since last save.
    pub fn save(document_id: DocumentId, app: &App, io: &mut dyn IO, kind: SaveKind) {
        let mut document = document_id.get_mut(app);
        let absolute_path = match &document.source {
            Source::File(SourceFile {
                absolute_path,
                last_save_time,
                ..
            }) if *last_save_time > document.last_modified_time => absolute_path.clone(),
            _ => return,
        };
        let create = kind == SaveKind::Explicit;
        match io.file_write(&absolute_path, &document.text, create) {
            Ok(mtime) => {
                if let Source::File(SourceFile {
                    last_load_mtime,
                    last_save_time,
                    deleted_since_last_save,
                    ..
                }) = &mut document.source
                {
                    *last_load_mtime = mtime;
                    *last_save_time = io.frame_start();
                    *deleted_since_last_save = false;
                }
            }
            Err(err) if kind == SaveKind::Auto && err.kind() == std::io::ErrorKind::NotFound => {
                if let Source::File(SourceFile {
                    deleted_since_last_save,
                    ..
                }) = &mut document.source
                {
                    *deleted_since_last_save = true;
                }
            }
            Err(err) => {
                eprintln!("error saving {}: {}", absolute_path.display(), err);
            }
        }
    }

    pub fn grid_from_offset(document_id: DocumentId, app: &App, offset: usize) -> [usize; 2] {
        document_id.get(app).grid_at_offset(offset)
    }

    pub fn line_range_from_offset(
        document_id: DocumentId,
        app: &App,
        offset: usize,
    ) -> std::ops::Range<usize> {
        document_id.get(app).line_range_at_offset(offset)
    }

    pub fn char_next(document_id: DocumentId, app: &App, offset: usize) -> Option<usize> {
        let document = document_id.get(app);
        if offset == document.text.len() {
            return None;
        }
        let (_, char_end, _) = document.text[offset..].char_indices().next().unwrap();
        Some(offset + char_end)
    }

    pub fn char_prev(document_id: DocumentId, app: &App, offset: usize) -> Option<usize> {
        let document = document_id.get(app);
        if offset == 0 {
            return None;
        }
        // We can't directly iter backwards through potentially invalid utf8, but we
        // can go forwards from the start of the line.
        let line_start = document.line_range_at_offset(offset).start;
        if line_start == offset {
            // Previous character is a \n
            return Some(line_start - 1);
        }
        for (char_start, char_end, _) in document.text[line_start..].char_indices() {
            if line_start + char_end == offset {
                return Some(line_start + char_start);
            }
        }
        unreachable!()
    }

    pub fn assert_invariants(&self) {
        assert_eq!(
            self.text.chars().filter(|c| *c == '\n').count(),
            self.newlines.len(),
        );
        for offset in &self.newlines {
            assert_eq!(self.text[*offset..].chars().next().unwrap(), '\n');
        }
    }

    fn grid_at_offset(&self, offset: usize) -> [usize; 2] {
        let line = self.newlines.partition_point(|&nl| nl < offset);
        if line == 0 {
            [self.text[0..offset].chars().count(), 0]
        } else {
            [
                self.text[self.newlines[line - 1] + 1..offset]
                    .chars()
                    .count(),
                line,
            ]
        }
    }

    fn line_range_at_offset(&self, offset: usize) -> std::ops::Range<usize> {
        let line = self.grid_at_offset(offset)[1];
        if line == 0 {
            0..{
                if self.newlines.is_empty() {
                    self.text.len()
                } else {
                    self.newlines[0]
                }
            }
        } else {
            (self.newlines[line - 1] + 1)..{
                if line < self.newlines.len() {
                    self.newlines[line]
                } else {
                    self.text.len()
                }
            }
        }
    }
}

impl SourceFile {
    /// If the file's mtime has advanced past `last_load_mtime` and the doc is
    /// not modified, reload its contents.
    fn refresh_from_disk(&mut self, io: &mut dyn IO) -> Option<BString> {
        let Ok(mtime) = io.file_mtime(&self.absolute_path) else {
            return None;
        };
        if mtime <= self.last_load_mtime {
            return None;
        }
        let contents = match io.file_read(&self.absolute_path) {
            Ok(c) => c,
            Err(err) => {
                eprintln!("error reading {}: {}", self.absolute_path.display(), err);
                return None;
            }
        };
        self.last_load_mtime = mtime;
        Some(contents.into())
    }
}

impl Edit {
    fn assert_invariants(edits: &[Edit], text: &BStr) {
        for edit in edits {
            assert!(edit.offset <= text.len(), "Edit out of bounds");
            match edit.kind {
                EditKind::Insert => {}
                EditKind::Delete => {
                    assert!(
                        edit.offset + edit.text.len() <= text.len(),
                        "Delete out of bounds"
                    );
                    assert_eq!(
                        edit.text,
                        text[edit.offset..edit.offset + edit.text.len()],
                        "Delete text doesn't match document text"
                    );
                }
            }
        }
        for pair in edits.windows(2) {
            assert!(pair[0].offset <= pair[1].offset, "Edits are out of order");
            match pair[0].kind {
                EditKind::Insert => {}
                EditKind::Delete => {
                    assert!(
                        pair[0].offset + pair[0].text.len() <= pair[1].offset,
                        "Edits overlap"
                    );
                }
            }
        }
    }

    /// Sort by offset and truncate overlapping deletions. Inserts whose
    /// offset falls inside a previously deleted region are moved forward
    /// past that region.
    pub fn coalesce(edits: &mut Vec<Edit>) {
        edits.sort_by_key(|e| e.offset);

        let mut consumed_up_to: usize = 0;
        let mut i = 0;
        while i < edits.len() {
            match edits[i].kind {
                EditKind::Insert => {
                    if edits[i].offset < consumed_up_to {
                        edits[i].offset = consumed_up_to;
                    }
                    consumed_up_to = edits[i].offset;
                }
                EditKind::Delete => {
                    let end = edits[i].offset + edits[i].text.len();
                    if edits[i].offset < consumed_up_to {
                        if end <= consumed_up_to {
                            edits.remove(i);
                            continue;
                        }
                        let skip = consumed_up_to - edits[i].offset;
                        edits[i].offset = consumed_up_to;
                        edits[i].text = edits[i].text[skip..].into();
                    }
                    consumed_up_to = edits[i].offset + edits[i].text.len();
                }
            }
            i += 1;
        }
    }
}

impl OffsetDiff {
    fn assert_invariants(&self) {
        assert_eq!(self.offsets_old.len() + 1, self.offsets_new.len());
        assert_eq!(self.offsets_new.len(), self.deleted.len());
        for pair in self.offsets_old.windows(2) {
            assert!(pair[0] < pair[1]);
        }
    }

    fn from_edits(edits: &[Edit], len_old: usize) -> Self {
        let mut offsets_old: Vec<usize> = Vec::new();
        let mut offsets_new: Vec<usize> = vec![0]; // F(0) = 0
        let mut deleted: Vec<bool> = vec![false];
        let mut cum_shift: isize = 0;

        for edit in edits {
            match edit.kind {
                EditKind::Insert => {
                    offsets_old.push(edit.offset);
                    cum_shift += edit.text.len() as isize;
                    offsets_new.push(((edit.offset as isize) + cum_shift) as usize);
                    deleted.push(false);
                }
                EditKind::Delete => {
                    let end = edit.offset + edit.text.len();
                    offsets_old.push(edit.offset);
                    let start_new = ((edit.offset as isize) + cum_shift) as usize;
                    offsets_new.push(start_new);
                    deleted.push(true); // offsets in (edit.offset, end) clamp to start_new

                    offsets_old.push(end);
                    cum_shift -= edit.text.len() as isize;
                    offsets_new.push(((end as isize) + cum_shift) as usize);
                    deleted.push(false);
                }
            }
        }

        // Final sentinel segment
        offsets_old.push(len_old);
        offsets_new.push(((len_old as isize) + cum_shift) as usize);
        deleted.push(false);

        // TODO fix this slop.
        // Dedup adjacent same offsets_old by removing the EARLIER occurrence.
        // The later occurrence reflects the state after the later edit at that offset.
        let mut i = 0;
        while i + 1 < offsets_old.len() {
            if offsets_old[i] == offsets_old[i + 1] {
                offsets_old.remove(i);
                offsets_new.remove(i + 1); // remove the segment STARTING at the duplicate
                deleted.remove(i + 1);
                // Don't increment i — re-check the new element at this offset
            } else {
                i += 1;
            }
        }

        let diff = OffsetDiff {
            offsets_old,
            offsets_new,
            deleted,
        };
        diff.assert_invariants();
        diff
    }

    pub fn apply(&self, offset: usize) -> usize {
        if self.offsets_new.is_empty() {
            return offset;
        }
        let i = self.offsets_old.partition_point(|&o| o <= offset);
        if self.deleted[i] {
            self.offsets_new[i]
        } else {
            let old_start = if i == 0 { 0 } else { self.offsets_old[i - 1] };
            offset + self.offsets_new[i] - old_start
        }
    }
}

// Char-based diff via the `similar` crate. Produces edits that transform
// `old` into `new`.
fn diff_text(old: &BStr, new: &BStr) -> Vec<Edit> {
    let diff = similar::TextDiff::from_chars(old.as_bytes(), new.as_bytes());
    let mut edits = Vec::new();
    let mut byte_offset: usize = 0;
    let mut hunk_offset: Option<usize> = None;
    let mut hunk_ins: Vec<u8> = Vec::new();
    let mut hunk_del: Vec<u8> = Vec::new();

    for change in diff.iter_all_changes() {
        let value: &[u8] = change.value();
        match change.tag() {
            similar::ChangeTag::Equal => {
                diff_text_flush(&mut edits, &mut hunk_offset, &mut hunk_ins, &mut hunk_del);
                byte_offset += value.len();
            }
            similar::ChangeTag::Delete => {
                if hunk_offset.is_none() {
                    hunk_offset = Some(byte_offset);
                }
                hunk_del.extend_from_slice(value);
                byte_offset += value.len();
            }
            similar::ChangeTag::Insert => {
                if hunk_offset.is_none() {
                    hunk_offset = Some(byte_offset);
                }
                hunk_ins.extend_from_slice(value);
            }
        }
    }
    diff_text_flush(&mut edits, &mut hunk_offset, &mut hunk_ins, &mut hunk_del);
    edits
}

fn diff_text_flush(
    edits: &mut Vec<Edit>,
    hunk_offset: &mut Option<usize>,
    hunk_ins: &mut Vec<u8>,
    hunk_del: &mut Vec<u8>,
) {
    let Some(offset) = hunk_offset.take() else {
        return;
    };
    // `similar` can emit adjacent per-char inserts/deletes for a single
    // replacement. Keep hunk buffers so those changes coalesce into one insert
    // and one delete at the same offset, which preserves Edit invariants.
    if !hunk_ins.is_empty() {
        edits.push(Edit {
            kind: EditKind::Insert,
            offset,
            text: std::mem::take(hunk_ins).into(),
        });
    }
    if !hunk_del.is_empty() {
        edits.push(Edit {
            kind: EditKind::Delete,
            offset,
            text: std::mem::take(hunk_del).into(),
        });
    }
}
