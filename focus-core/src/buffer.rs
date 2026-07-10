use std::mem::take;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use bstr::{BStr, BString, ByteSlice};

use crate::app::{App, IO};
use crate::map::{Map, MapKey};

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy, Debug)]
pub struct BufferId(pub(crate) usize);

impl MapKey for BufferId {
    fn index(self) -> usize {
        self.0
    }

    fn from_index(index: usize) -> Self {
        BufferId(index)
    }
}

pub struct Buffers {
    pub(crate) buffer_count: usize,

    source: Map<BufferId, Source>,
    text: Map<BufferId, BString>,
    newlines: Map<BufferId, Vec<usize>>,
    last_modified_time: Map<BufferId, Duration>,
    undos: Map<BufferId, Vec<Vec<Vec<Edit>>>>,
    doing: Map<BufferId, Vec<Vec<Edit>>>,
    redos: Map<BufferId, Vec<Vec<Vec<Edit>>>>,

    // Can be set by editor.
    pub(crate) last_center_offset: Map<BufferId, usize>,
}

pub(crate) enum Source {
    Scratch,
    File(SourceFile),
}

pub(crate) struct SourceFile {
    pub(crate) absolute_path: PathBuf,
    last_load_mtime: SystemTime,
    last_save_time: Duration,
    deleted_since_last_save: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SaveKind {
    Explicit,
    Auto,
}

#[derive(Clone, Debug)]
pub(crate) struct Edit {
    pub(crate) kind: EditKind,
    pub(crate) offset: usize,
    pub(crate) text: BString,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum EditKind {
    Insert,
    Delete,
}

pub(crate) struct OffsetDiff {
    offsets_old: Vec<usize>,
    offsets_new: Vec<usize>,
    deleted: Vec<bool>,
}

impl Buffers {
    pub(crate) fn new() -> Buffers {
        Buffers {
            buffer_count: 0,
            source: Map::new(),
            text: Map::new(),
            newlines: Map::new(),
            last_modified_time: Map::new(),
            undos: Map::new(),
            doing: Map::new(),
            redos: Map::new(),
            last_center_offset: Map::new(),
        }
    }

    pub fn keys(&self) -> impl Iterator<Item = BufferId> + '_ {
        (0..self.buffer_count).map(BufferId)
    }
}

pub(crate) fn scratch(app: &mut App) -> BufferId {
    insert(app, Source::Scratch)
}

pub fn from_file(app: &mut App, absolute_path: PathBuf) -> BufferId {
    insert(
        app,
        Source::File(SourceFile {
            absolute_path,
            last_load_mtime: SystemTime::UNIX_EPOCH,
            last_save_time: Duration::ZERO,
            deleted_since_last_save: false,
        }),
    )
}

fn insert(app: &mut App, source: Source) -> BufferId {
    let buffer_id = BufferId(app.buffers.buffer_count);
    app.buffers.buffer_count += 1;
    app.buffers.source.insert(buffer_id, source);
    app.buffers.text.insert(buffer_id, "".into());
    app.buffers.newlines.insert(buffer_id, vec![]);
    app.buffers
        .last_modified_time
        .insert(buffer_id, Duration::ZERO);
    app.buffers.undos.insert(buffer_id, vec![]);
    app.buffers.doing.insert(buffer_id, vec![]);
    app.buffers.redos.insert(buffer_id, vec![]);
    app.buffers.last_center_offset.insert(buffer_id, 0);
    buffer_id
}

pub(crate) fn assert_invariants(app: &App) {
    let buffers = &app.buffers;
    assert_eq!(buffers.source.len(), buffers.buffer_count);
    assert_eq!(buffers.text.len(), buffers.buffer_count);
    assert_eq!(buffers.newlines.len(), buffers.buffer_count);
    assert_eq!(buffers.last_modified_time.len(), buffers.buffer_count);
    assert_eq!(buffers.undos.len(), buffers.buffer_count);
    assert_eq!(buffers.doing.len(), buffers.buffer_count);
    assert_eq!(buffers.redos.len(), buffers.buffer_count);
    assert_eq!(buffers.last_center_offset.len(), buffers.buffer_count);
    for buffer_id in (0..buffers.buffer_count).map(BufferId) {
        buffers.source.get(buffer_id).assert_invariants();
        let text = buffers.text.get(buffer_id);
        let newlines = buffers.newlines.get(buffer_id);
        assert_eq!(text.chars().filter(|c| *c == '\n').count(), newlines.len());
        for offset in newlines {
            assert_eq!(text[*offset..].chars().next().unwrap(), '\n');
        }
        for undo in buffers.undos.get(buffer_id) {
            assert!(!undo.is_empty());
            for edits in undo {
                Edit::assert_invariants(edits, None);
            }
        }
        for edits in buffers.doing.get(buffer_id) {
            Edit::assert_invariants(edits, None);
        }
        for redo in buffers.redos.get(buffer_id) {
            assert!(!redo.is_empty());
            for edits in redo {
                Edit::assert_invariants(edits, None);
            }
        }
    }
}

impl BufferId {
    pub fn text(self, app: &App) -> &BStr {
        app.buffers.text.get(self).as_bstr()
    }

    pub(crate) fn source(self, app: &App) -> &Source {
        app.buffers.source.get(self)
    }

    pub(crate) fn tick(self, app: &mut App, io: &mut dyn IO) {
        // Maybe flush doing.
        let last_modified_time = *app.buffers.last_modified_time.get(self);
        if app.frame_start - last_modified_time > Duration::from_secs(1) {
            self.flush_doing(app);
        }

        // Maybe reload.
        let frame_start = app.frame_start;
        let last_modified_time = *app.buffers.last_modified_time.get(self);
        let text = match app.buffers.source.get_mut(self) {
            Source::File(source) if !(last_modified_time > source.last_save_time) => {
                source.load(io, frame_start)
            }
            _ => None,
        };
        if let Some(text) = text {
            self.replace(app, text.as_bstr());
        }
    }

    pub(crate) fn replace(self, app: &mut App, text: &BStr) {
        let edits = diff_text(self.text(app).as_bstr(), text);
        self.apply_edits(app, &edits);
    }

    pub(crate) fn apply_edits(self, app: &mut App, edits: &[Edit]) {
        if edits.is_empty() {
            return;
        }

        self.apply_edits_raw(app, edits);

        app.buffers.doing.get_mut(self).push(edits.to_vec());
        app.buffers.redos.get_mut(self).clear();
    }

    fn apply_edits_raw(self, app: &mut App, edits: &[Edit]) {
        assert!(!edits.is_empty());

        let frame_start = app.frame_start;
        let len_old = {
            let text = app.buffers.text.get_mut(self);
            Edit::assert_invariants(edits, Some(text.as_bstr()));

            let len_old = text.len();

            // TODO This can be made way more efficient, so that common cases don't have to allocate a whole new text.
            let mut text_new = BString::new(Vec::with_capacity(text.len()));
            let mut offset = 0;
            for edit in edits {
                text_new.extend_from_slice(&text[offset..edit.offset]);
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
            text_new.extend_from_slice(&text[offset..]);
            *text = text_new;
            len_old
        };

        // TODO This can be made way more efficient, so that common cases don't have to iterate over the whole text.
        let mut newlines = Vec::new();
        for (char_start, _, char) in app.buffers.text.get(self).char_indices() {
            if char == '\n' {
                newlines.push(char_start);
            }
        }
        *app.buffers.newlines.get_mut(self) = newlines;

        let diff = OffsetDiff::from_edits(edits, len_old);

        *app.buffers.last_modified_time.get_mut(self) = frame_start;

        let editor_ids: Vec<_> = app
            .editors
            .buffer_id
            .iter()
            .filter_map(|(editor_id, buffer_id)| (*buffer_id == self).then_some(editor_id))
            .collect();
        for editor_id in &editor_ids {
            editor_id.handle_edits(app, &diff);
        }

        let mut page_ids = Vec::new();
        for (page_id, editor_ids_for_page) in app.pages.editor_ids.iter() {
            for (editor_ix, editor_id) in editor_ids_for_page.iter().enumerate() {
                if editor_ids.contains(editor_id) {
                    page_ids.push((page_id, editor_ix));
                }
            }
        }
        for (page_id, editor_ix) in page_ids {
            page_id.handle_edits(app, editor_ix, &diff);
        }
    }

    /// Save to disk. Explicit saves create the file if missing; autosaves
    /// treat NotFound as an external deletion.
    /// No-op for Scratch sources or when not modified since last save.
    pub(crate) fn save(self, app: &mut App, io: &mut dyn IO, kind: SaveKind) {
        let frame_start = app.frame_start;
        let last_modified_time = *app.buffers.last_modified_time.get(self);
        let absolute_path = match app.buffers.source.get(self) {
            Source::File(SourceFile {
                absolute_path,
                last_save_time,
                ..
            }) if last_modified_time > *last_save_time => absolute_path.clone(),
            _ => return,
        };
        let contents = app.buffers.text.get(self).clone();
        let create = kind == SaveKind::Explicit;
        match io.file_write(&absolute_path, &contents, create) {
            Ok(mtime) => {
                if let Source::File(SourceFile {
                    last_load_mtime,
                    last_save_time,
                    deleted_since_last_save,
                    ..
                }) = app.buffers.source.get_mut(self)
                {
                    *last_load_mtime = mtime;
                    *last_save_time = frame_start;
                    *deleted_since_last_save = false;
                }
            }
            Err(err) if kind == SaveKind::Auto && err.kind() == std::io::ErrorKind::NotFound => {
                if let Source::File(SourceFile {
                    deleted_since_last_save,
                    ..
                }) = app.buffers.source.get_mut(self)
                {
                    *deleted_since_last_save = true;
                }
            }
            Err(err) => {
                eprintln!("error saving {}: {}", absolute_path.display(), err);
            }
        }
    }

    pub(crate) fn grid_from_offset(self, app: &App, offset: usize) -> [usize; 2] {
        let text = app.buffers.text.get(self);
        let newlines = app.buffers.newlines.get(self);
        let line = newlines.partition_point(|&nl| nl < offset);
        if line == 0 {
            [text[0..offset].chars().count(), 0]
        } else {
            [text[newlines[line - 1] + 1..offset].chars().count(), line]
        }
    }

    pub(crate) fn line_range_from_offset(self, app: &App, offset: usize) -> std::ops::Range<usize> {
        let line = self.grid_from_offset(app, offset)[1];
        let text = app.buffers.text.get(self);
        let newlines = app.buffers.newlines.get(self);
        if line == 0 {
            0..{
                if newlines.is_empty() {
                    text.len()
                } else {
                    newlines[0]
                }
            }
        } else {
            (newlines[line - 1] + 1)..{
                if line < newlines.len() {
                    newlines[line]
                } else {
                    text.len()
                }
            }
        }
    }

    pub(crate) fn char_next(self, app: &App, offset: usize) -> Option<usize> {
        let text = app.buffers.text.get(self);
        if offset == text.len() {
            return None;
        }
        let (_, char_end, _) = text[offset..].char_indices().next().unwrap();
        Some(offset + char_end)
    }

    pub(crate) fn char_prev(self, app: &App, offset: usize) -> Option<usize> {
        let text = app.buffers.text.get(self);
        if offset == 0 {
            return None;
        }
        let (char_start, _, _) = text[..offset].char_indices().next_back().unwrap();
        Some(char_start)
    }

    pub(crate) fn flush_doing(self, app: &mut App) {
        let doing = app.buffers.doing.get_mut(self);
        if doing.is_empty() {
            return;
        }
        let doing = take(doing);
        app.buffers.undos.get_mut(self).push(doing);
    }

    pub(crate) fn undo(self, app: &mut App) -> Option<usize> {
        self.flush_doing(app);
        let undo = app.buffers.undos.get_mut(self).pop()?;
        let offset = undo.first().unwrap().last().unwrap().offset;
        let mut redo = vec![];
        for mut edits in undo.into_iter().rev() {
            Edit::undo(&mut edits);
            self.apply_edits_raw(app, &edits);
            redo.push(edits);
        }
        app.buffers.redos.get_mut(self).push(redo);
        Some(offset)
    }

    pub(crate) fn redo(self, app: &mut App) -> Option<usize> {
        self.flush_doing(app);
        let redo = app.buffers.redos.get_mut(self).pop()?;
        let offset = redo.first().unwrap().last().unwrap().offset;
        let mut undo = vec![];
        for mut edits in redo.into_iter().rev() {
            Edit::undo(&mut edits);
            self.apply_edits_raw(app, &edits);
            undo.push(edits);
        }
        app.buffers.undos.get_mut(self).push(undo);
        Some(offset)
    }
}

impl Source {
    pub fn assert_invariants(&self) {
        match self {
            Source::Scratch => {}
            Source::File(file) => file.assert_invariants(),
        }
    }
}

impl SourceFile {
    pub fn assert_invariants(&self) {
        assert!(self.absolute_path.is_absolute());
    }

    /// If the file's mtime has advanced past `last_load_mtime` and the buffer is
    /// not modified, reload its contents.
    fn load(&mut self, io: &mut dyn IO, frame_start: Duration) -> Option<BString> {
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
        self.last_save_time = frame_start;
        Some(contents.into())
    }
}

impl Edit {
    fn assert_invariants(edits: &[Edit], text: Option<&BStr>) {
        assert!(!edits.is_empty());
        if let Some(text) = text {
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
                            "Delete text doesn't match buffer text"
                        );
                    }
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
    pub(crate) fn coalesce(edits: &mut Vec<Edit>) {
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

    pub(crate) fn undo(edits: &mut Vec<Edit>) {
        let mut shift: isize = 0;
        for edit in edits {
            let len = edit.text.len() as isize;
            edit.offset = (edit.offset as isize + shift) as usize;
            match edit.kind {
                EditKind::Insert => {
                    edit.kind = EditKind::Delete;
                    shift += len;
                }
                EditKind::Delete => {
                    edit.kind = EditKind::Insert;
                    shift -= len;
                }
            }
        }
    }
}

impl OffsetDiff {
    fn assert_invariants(&self) {
        assert_eq!(self.offsets_old.len() + 1, self.offsets_new.len());
        assert_eq!(self.offsets_new.len(), self.deleted.len());
        for pair in self.offsets_old.windows(2) {
            assert!(pair[0] <= pair[1]);
        }
        for pair in self.offsets_new.windows(2) {
            assert!(pair[0] <= pair[1]);
        }
    }

    fn from_edits(edits: &[Edit], len_old: usize) -> Self {
        let mut offsets_old: Vec<usize> = Vec::new();
        let mut offsets_new: Vec<usize> = vec![0];
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
                    deleted.push(true);

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

        let diff = OffsetDiff {
            offsets_old,
            offsets_new,
            deleted,
        };
        diff.assert_invariants();
        diff
    }

    pub(crate) fn apply(&self, offset: usize) -> usize {
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

fn diff_text(old: &BStr, new: &BStr) -> Vec<Edit> {
    let diff = similar::TextDiff::configure()
        .algorithm(similar::Algorithm::Histogram)
        .diff_words(old.as_bytes(), new.as_bytes());

    // DiffOp ranges are token indices, not byte offsets. Build a prefix sum of
    // token byte lengths so we can translate a token index into a byte offset.
    let byte_offsets = |tokens: &mut dyn Iterator<Item = &[u8]>| {
        let mut offsets = vec![0];
        let mut acc = 0;
        for token in tokens {
            acc += token.len();
            offsets.push(acc);
        }
        offsets
    };
    let old_offsets = byte_offsets(&mut diff.iter_old_slices());
    let new_offsets = byte_offsets(&mut diff.iter_new_slices());

    let mut edits = Vec::new();
    for op in diff.ops() {
        let (tag, old_range, new_range) = op.as_tag_tuple();
        let old_start = old_offsets[old_range.start];
        let old_end = old_offsets[old_range.end];
        let new_start = new_offsets[new_range.start];
        let new_end = new_offsets[new_range.end];
        match tag {
            similar::DiffTag::Equal => {}
            similar::DiffTag::Delete => {
                edits.push(Edit {
                    kind: EditKind::Delete,
                    offset: old_start,
                    text: old[old_start..old_end].into(),
                });
            }
            similar::DiffTag::Insert => {
                edits.push(Edit {
                    kind: EditKind::Insert,
                    offset: old_start,
                    text: new[new_start..new_end].into(),
                });
            }
            similar::DiffTag::Replace => {
                edits.push(Edit {
                    kind: EditKind::Insert,
                    offset: old_start,
                    text: new[new_start..new_end].into(),
                });
                edits.push(Edit {
                    kind: EditKind::Delete,
                    offset: old_start,
                    text: old[old_start..old_end].into(),
                });
            }
        }
    }

    edits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply_text(text: &[u8], edits: &[Edit]) -> BString {
        Edit::assert_invariants(edits, Some(text.as_bstr()));

        let mut text_new = BString::new(Vec::new());
        let mut offset = 0;
        for edit in edits {
            text_new.extend_from_slice(&text[offset..edit.offset]);
            offset = edit.offset;
            match edit.kind {
                EditKind::Insert => text_new.extend_from_slice(&edit.text),
                EditKind::Delete => offset += edit.text.len(),
            }
        }
        text_new.extend_from_slice(&text[offset..]);
        text_new
    }

    #[test]
    fn diff_text_coalesces_multi_word_changes() {
        let old = "the quick brown fox".as_bytes().as_bstr();
        let new = "the slow fox".as_bytes().as_bstr();
        let edits = diff_text(old, new);

        // The whole changed region ("quick brown" -> "slow") is a single
        // contiguous run of differing tokens, so it coalesces into one
        // insert/delete pair rather than one edit per word.
        assert_eq!(edits.len(), 2);
        assert!(matches!(edits[0].kind, EditKind::Insert));
        assert_eq!(edits[0].offset, 4);
        assert_eq!(edits[0].text, "slow");
        assert!(matches!(edits[1].kind, EditKind::Delete));
        assert_eq!(edits[1].offset, 4);
        assert_eq!(edits[1].text, "quick brown");
        assert_eq!(apply_text(old, &edits), new);
    }

    #[test]
    fn diff_text_uses_byte_offsets_for_multibyte_chars() {
        // "café" is 5 bytes (é is 2 bytes), the space is byte 5, so the
        // changed word "résumé" starts at byte 6 even though it is token 2.
        let old = "café résumé".as_bytes().as_bstr();
        let new = "café output".as_bytes().as_bstr();
        let edits = diff_text(old, new);

        assert_eq!(edits.len(), 2);
        assert!(matches!(edits[0].kind, EditKind::Insert));
        assert_eq!(edits[0].offset, 6);
        assert_eq!(edits[0].text, "output");
        assert!(matches!(edits[1].kind, EditKind::Delete));
        assert_eq!(edits[1].offset, 6);
        assert_eq!(edits[1].text, "résumé");
        assert_eq!(apply_text(old, &edits), new);
    }

    #[test]
    fn undo_inverts_replacement_edits() {
        let old = b"abcdef";
        let mut edits = vec![
            Edit {
                kind: EditKind::Insert,
                offset: 1,
                text: "XY".into(),
            },
            Edit {
                kind: EditKind::Delete,
                offset: 1,
                text: "bcd".into(),
            },
        ];
        let new = apply_text(old, &edits);

        Edit::undo(&mut edits);

        assert_eq!(new, "aXYef");
        assert_eq!(apply_text(&new, &edits), old.as_slice());
    }

    #[test]
    fn undo_inverts_mixed_offset_edits() {
        let old = b"abcdef";
        let mut edits = vec![
            Edit {
                kind: EditKind::Delete,
                offset: 1,
                text: "bc".into(),
            },
            Edit {
                kind: EditKind::Insert,
                offset: 4,
                text: "X".into(),
            },
        ];
        let new = apply_text(old, &edits);

        Edit::undo(&mut edits);

        assert_eq!(new, "adXef");
        assert_eq!(apply_text(&new, &edits), old.as_slice());
    }
}
