use std::mem::take;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use bstr::{BStr, BString, ByteSlice};

use crate::app::{App, IO};
use crate::map::Map;

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy, Debug)]
pub struct BufferId(pub(crate) usize);

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
    /// Editable, not backed by a file: input fields, scratch pages.
    Scratch,
    /// Editable, backed by a file on disk.
    File(SourceFile),
    /// Written only by the editor itself: picker lists, previews, runner
    /// output, status bars. Ignores user input and keeps no undo history.
    Generated,
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

pub(crate) fn generated(app: &mut App) -> BufferId {
    insert(app, Source::Generated)
}

/// Return the existing buffer for `absolute_path`, or create one.
pub fn from_file(app: &mut App, io: &mut dyn IO, absolute_path: PathBuf) -> BufferId {
    // Callers resolve paths against a known dir; a relative path here would
    // silently read and save relative to the editor's own cwd.
    assert!(
        absolute_path.is_absolute(),
        "buffer path must be absolute: {:?}",
        absolute_path
    );
    // Buffers are keyed by path, so `focus ../notes.txt` and
    // `focus /home/j/notes.txt` have to arrive here spelled the same way -
    // otherwise one file gets two buffers, and whichever saves last wins.
    let absolute_path = io.canonical_path(&absolute_path);
    for buffer_id in app.buffers.keys() {
        let Source::File(source) = &app.buffers.source[buffer_id] else {
            continue;
        };
        if source.absolute_path.as_path() == absolute_path.as_path() {
            return buffer_id;
        }
    }

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

/// A new buffer of the same kind holding a copy of `buffer_id`'s text and
/// edit history. Used to duplicate a page: each page owns its input fields
/// and its generated buffers, so a copy of the page needs its own.
/// File-backed buffers are never copied - two buffers over one path would
/// save over each other - so pages share those instead.
pub(crate) fn copy(app: &mut App, buffer_id: BufferId) -> BufferId {
    let source = match buffer_id.source(app) {
        Source::Scratch => Source::Scratch,
        Source::Generated => Source::Generated,
        Source::File(SourceFile { absolute_path, .. }) => {
            panic!("can't copy the file buffer for {:?}", absolute_path)
        }
    };
    let text = app.buffers.text[buffer_id].clone();
    let newlines = app.buffers.newlines[buffer_id].clone();
    let last_modified_time = app.buffers.last_modified_time[buffer_id];
    let undos = app.buffers.undos[buffer_id].clone();
    let doing = app.buffers.doing[buffer_id].clone();
    let redos = app.buffers.redos[buffer_id].clone();
    let last_center_offset = app.buffers.last_center_offset[buffer_id];
    let copy_id = insert(app, source);
    app.buffers.text[copy_id] = text;
    app.buffers.newlines[copy_id] = newlines;
    app.buffers.last_modified_time[copy_id] = last_modified_time;
    app.buffers.undos[copy_id] = undos;
    app.buffers.doing[copy_id] = doing;
    app.buffers.redos[copy_id] = redos;
    app.buffers.last_center_offset[copy_id] = last_center_offset;
    copy_id
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
        buffers.source[buffer_id].assert_invariants();
        let text = &buffers.text[buffer_id];
        let newlines = &buffers.newlines[buffer_id];
        assert_eq!(text.chars().filter(|c| *c == '\n').count(), newlines.len());
        for offset in newlines {
            assert_eq!(text[*offset..].chars().next().unwrap(), '\n');
        }
        if !buffer_id.is_editable(app) {
            // Generated buffers record no history: nothing undoes a picker
            // list or a command's output, and keeping one would grow without
            // bound as the page rewrites the buffer every tick.
            assert!(buffers.undos[buffer_id].is_empty());
            assert!(buffers.doing[buffer_id].is_empty());
            assert!(buffers.redos[buffer_id].is_empty());
        }
        for undo in &buffers.undos[buffer_id] {
            assert!(!undo.is_empty());
            for edits in undo {
                Edit::assert_invariants(edits, None);
            }
        }
        for edits in &buffers.doing[buffer_id] {
            Edit::assert_invariants(edits, None);
        }
        for redo in &buffers.redos[buffer_id] {
            assert!(!redo.is_empty());
            for edits in redo {
                Edit::assert_invariants(edits, None);
            }
        }
    }
}

impl BufferId {
    pub fn text(self, app: &App) -> &BStr {
        app.buffers.text[self].as_bstr()
    }

    pub(crate) fn source(self, app: &App) -> &Source {
        &app.buffers.source[self]
    }

    /// Whether user input can edit this buffer. Generated buffers are
    /// rewritten by their page every tick, so typing into one would be
    /// undone by the next frame; the editor ignores edits to them instead.
    pub(crate) fn is_editable(self, app: &App) -> bool {
        !matches!(self.source(app), Source::Generated)
    }

    pub(crate) fn path(self, app: &App) -> Option<PathBuf> {
        match self.source(app) {
            Source::Scratch | Source::Generated => None,
            Source::File(SourceFile { absolute_path, .. }) => Some(absolute_path.clone()),
        }
    }

    pub(crate) fn tick(self, app: &mut App, io: &mut dyn IO) {
        // Generated buffers have no history to flush and no file to reload
        // from: their page rewrites them every tick instead.
        if !self.is_editable(app) {
            return;
        }

        // Maybe flush doing.
        let last_modified_time = app.buffers.last_modified_time[self];
        if app.frame_start - last_modified_time > Duration::from_secs(1) {
            self.flush_doing(app);
        }

        // Maybe reload.
        let frame_start = app.frame_start;
        let last_modified_time = app.buffers.last_modified_time[self];
        let (text, first_load) = match &mut app.buffers.source[self] {
            Source::File(source) if last_modified_time <= source.last_save_time => {
                let first_load = source.last_load_mtime == SystemTime::UNIX_EPOCH;
                (source.load(io, frame_start), first_load)
            }
            _ => (None, false),
        };
        if let Some(text) = text {
            if first_load {
                // The first load isn't an edit: it replaces whatever was
                // typed before the file arrived, and drops that typing's
                // undo history along with it (undoing it against the loaded
                // text would delete the wrong bytes). Open at the top rather
                // than mapping cursors through the empty->contents diff.
                self.replace_raw(app, text.as_bstr());
                app.buffers.undos[self].clear();
                app.buffers.doing[self].clear();
                app.buffers.redos[self].clear();
                let editor_ids: Vec<_> = app
                    .editors
                    .buffer_id
                    .iter()
                    .filter_map(|(editor_id, buffer_id)| (*buffer_id == self).then_some(editor_id))
                    .collect();
                for editor_id in editor_ids {
                    editor_id.cursor_reset(app);
                }
            } else {
                self.replace(app, text.as_bstr());
            }
        }
    }

    /// Replace the buffer's text. Editable buffers use an undoable diff so
    /// cursors and selections survive the change; generated buffers replace
    /// the whole text without recording history.
    pub(crate) fn replace(self, app: &mut App, text: &BStr) {
        if self.is_editable(app) {
            let edits = diff_text(self.text(app).as_bstr(), text);
            self.apply_edits(app, &edits);
            return;
        }

        self.replace_raw(app, text);
    }

    /// Replace the whole buffer without diffing or recording history. Initial
    /// file loads and generated buffers do not need an edit-shaped change.
    fn replace_raw(self, app: &mut App, text: &BStr) {
        let old = self.text(app);
        if old == text {
            return;
        }
        let mut edits = Vec::new();
        if !text.is_empty() {
            edits.push(Edit {
                kind: EditKind::Insert,
                offset: 0,
                text: text.into(),
            });
        }
        if !old.is_empty() {
            edits.push(Edit {
                kind: EditKind::Delete,
                offset: 0,
                text: old.into(),
            });
        }
        self.apply_edits_raw(app, &edits);
    }

    /// Append generated text (eg command output) without recording undo.
    pub(crate) fn append(self, app: &mut App, text: &BStr) {
        assert!(!self.is_editable(app), "append to an editable buffer");
        if text.is_empty() {
            return;
        }
        let offset = self.text(app).len();
        self.apply_edits_raw(
            app,
            &[Edit {
                kind: EditKind::Insert,
                offset,
                text: text.into(),
            }],
        );
    }

    /// Drop text from the start so at most `max_len` bytes remain, cutting
    /// at a line boundary where there is one. Generated output only.
    pub(crate) fn trim_front_to(self, app: &mut App, max_len: usize) {
        assert!(!self.is_editable(app), "trim of an editable buffer");
        let len = self.text(app).len();
        if len <= max_len {
            return;
        }
        let cut = len - max_len;
        let cut = match self.text(app)[cut..].find_byte(b'\n') {
            Some(ix) => cut + ix + 1,
            None => cut,
        };
        let removed: BString = self.text(app)[..cut].into();
        self.apply_edits_raw(
            app,
            &[Edit {
                kind: EditKind::Delete,
                offset: 0,
                text: removed,
            }],
        );
    }

    pub(crate) fn apply_edits(self, app: &mut App, edits: &[Edit]) {
        // The editor gates every mutating key on the buffer being editable,
        // so reaching here with a generated one means a path was missed.
        assert!(self.is_editable(app), "undoable edit to a generated buffer");
        if edits.is_empty() {
            return;
        }

        self.apply_edits_raw(app, edits);

        app.buffers.doing[self].push(edits.to_vec());
        app.buffers.redos[self].clear();
    }

    fn apply_edits_raw(self, app: &mut App, edits: &[Edit]) {
        assert!(!edits.is_empty());

        let frame_start = app.frame_start;
        let text = &mut app.buffers.text[self];
        Edit::assert_invariants(edits, Some(text.as_bstr()));
        let len_old = text.len();
        let diff = OffsetDiff::from_edits(edits, len_old);

        // A single insert at the end is the streamed-output case: extend in
        // place and scan only the new text for newlines. This is a property
        // of the edits, not of the offsets, so read it from the edits.
        let appended = match edits {
            [
                Edit {
                    kind: EditKind::Insert,
                    offset,
                    text,
                },
            ] if *offset == len_old => Some(text),
            _ => None,
        };
        if let Some(new_text) = appended {
            text.extend_from_slice(new_text);
            let newlines = &mut app.buffers.newlines[self];
            newlines.extend(new_text.find_iter(b"\n").map(|ix| len_old + ix));
        } else {
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
            // `\n` is a single byte in any encoding this editor sees, so a
            // byte scan finds the same offsets as a char scan.
            app.buffers.newlines[self] = app.buffers.text[self].find_iter(b"\n").collect();
        }

        app.buffers.last_modified_time[self] = frame_start;

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
        let last_modified_time = app.buffers.last_modified_time[self];
        let create = kind == SaveKind::Explicit;
        let write_result = {
            let Source::File(SourceFile {
                absolute_path,
                last_save_time,
                ..
            }) = &app.buffers.source[self]
            else {
                return;
            };
            if last_modified_time <= *last_save_time {
                return;
            }
            io.file_write(absolute_path, &app.buffers.text[self], create)
        };
        match write_result {
            Ok(mtime) => {
                if let Source::File(SourceFile {
                    last_load_mtime,
                    last_save_time,
                    deleted_since_last_save,
                    ..
                }) = &mut app.buffers.source[self]
                {
                    *last_load_mtime = mtime;
                    *last_save_time = frame_start;
                    *deleted_since_last_save = false;
                }
                app.save_count += 1;
            }
            Err(err) if kind == SaveKind::Auto && err.kind() == std::io::ErrorKind::NotFound => {
                if let Source::File(SourceFile {
                    deleted_since_last_save,
                    ..
                }) = &mut app.buffers.source[self]
                {
                    *deleted_since_last_save = true;
                }
            }
            Err(err) => {
                let Source::File(SourceFile { absolute_path, .. }) = &app.buffers.source[self]
                else {
                    unreachable!();
                };
                crate::log!(io, "error saving {}: {}", absolute_path.display(), err);
            }
        }
    }

    pub(crate) fn grid_from_offset(self, app: &App, offset: usize) -> [usize; 2] {
        let text = &app.buffers.text[self];
        let newlines = &app.buffers.newlines[self];
        let line = newlines.partition_point(|&nl| nl < offset);
        let line_start = if line == 0 { 0 } else { newlines[line - 1] + 1 };
        [text[line_start..offset].chars().count(), line]
    }

    /// Inverse of grid_from_offset: the byte offset of the char at
    /// `[col, line]`, clamped to the end of the line / buffer.
    pub(crate) fn offset_from_grid(self, app: &App, grid: [usize; 2]) -> usize {
        let text = &app.buffers.text[self];
        let newlines = &app.buffers.newlines[self];
        let [col, line] = grid;
        let line = line.min(newlines.len());
        let start = if line == 0 { 0 } else { newlines[line - 1] + 1 };
        let end = newlines.get(line).copied().unwrap_or(text.len());
        match text[start..end].char_indices().nth(col) {
            Some((char_start, _, _)) => start + char_start,
            None => end,
        }
    }

    pub(crate) fn line_range_from_offset(self, app: &App, offset: usize) -> std::ops::Range<usize> {
        let line = self.grid_from_offset(app, offset)[1];
        let text = &app.buffers.text[self];
        let newlines = &app.buffers.newlines[self];
        let start = if line == 0 { 0 } else { newlines[line - 1] + 1 };
        let end = newlines.get(line).copied().unwrap_or(text.len());
        start..end
    }

    pub(crate) fn char_next(self, app: &App, offset: usize) -> Option<usize> {
        let text = &app.buffers.text[self];
        if offset == text.len() {
            return None;
        }
        let (_, char_end, _) = text[offset..].char_indices().next().unwrap();
        Some(offset + char_end)
    }

    pub(crate) fn char_prev(self, app: &App, offset: usize) -> Option<usize> {
        let text = &app.buffers.text[self];
        if offset == 0 {
            return None;
        }
        let (char_start, _, _) = text[..offset].char_indices().next_back().unwrap();
        Some(char_start)
    }

    pub(crate) fn flush_doing(self, app: &mut App) {
        let doing = &mut app.buffers.doing[self];
        if doing.is_empty() {
            return;
        }
        let doing = take(doing);
        app.buffers.undos[self].push(doing);
    }

    pub(crate) fn undo(self, app: &mut App) -> Option<usize> {
        self.flush_doing(app);
        let undo = app.buffers.undos[self].pop()?;
        let offset = undo.first().unwrap().last().unwrap().offset;
        let mut redo = vec![];
        for mut edits in undo.into_iter().rev() {
            Edit::undo(&mut edits);
            self.apply_edits_raw(app, &edits);
            redo.push(edits);
        }
        app.buffers.redos[self].push(redo);
        Some(offset)
    }

    pub(crate) fn redo(self, app: &mut App) -> Option<usize> {
        self.flush_doing(app);
        let redo = app.buffers.redos[self].pop()?;
        let offset = redo.first().unwrap().last().unwrap().offset;
        let mut undo = vec![];
        for mut edits in redo.into_iter().rev() {
            Edit::undo(&mut edits);
            self.apply_edits_raw(app, &edits);
            undo.push(edits);
        }
        app.buffers.undos[self].push(undo);
        Some(offset)
    }
}

impl Source {
    pub fn assert_invariants(&self) {
        match self {
            Source::Scratch | Source::Generated => {}
            Source::File(file) => file.assert_invariants(),
        }
    }
}

impl SourceFile {
    pub fn assert_invariants(&self) {
        assert!(
            self.absolute_path.is_absolute(),
            "not absolute: {:?}",
            self.absolute_path
        );
    }

    /// If the file's mtime has advanced past `last_load_mtime` and the buffer is
    /// not modified, reload its contents.
    fn load(&mut self, io: &mut dyn IO, frame_start: Duration) -> Option<BString> {
        let mtime = io.file_mtime(&self.absolute_path).ok()?;
        if mtime <= self.last_load_mtime {
            return None;
        }
        let contents = io.file_read(&self.absolute_path).ok()?;
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

    /// Offsets below this are left where they are by the diff, because
    /// nothing before the first edit can move. Callers tracking offsets
    /// use it to skip work that would map them to themselves.
    pub(crate) fn unchanged_before(&self) -> usize {
        // from_edits always pushes a final sentinel, so this is never empty.
        self.offsets_old[0]
    }

    /// Map an old offset to a new offset. An insert at exactly `offset`
    /// lands before it, so the mapped offset follows the inserted text.
    pub(crate) fn apply(&self, offset: usize) -> usize {
        let i = self.offsets_old.partition_point(|&o| o <= offset);
        self.apply_segment(i, offset)
    }

    /// Like `apply`, but an insert at exactly `offset` lands after it, so
    /// the mapped offset stays before the inserted text. Use for the end
    /// of a range, so typing just after the range doesn't extend it.
    pub(crate) fn apply_before(&self, offset: usize) -> usize {
        let i = self.offsets_old.partition_point(|&o| o < offset);
        self.apply_segment(i, offset)
    }

    fn apply_segment(&self, i: usize, offset: usize) -> usize {
        if self.deleted[i] {
            self.offsets_new[i]
        } else {
            let old_start = if i == 0 { 0 } else { self.offsets_old[i - 1] };
            offset + self.offsets_new[i] - old_start
        }
    }

    /// Map a range: the start moves past inserts at the start, the end
    /// stays before inserts at the end. Returns None if nothing of the
    /// range survives.
    pub(crate) fn apply_range(
        &self,
        range: std::ops::Range<usize>,
    ) -> Option<std::ops::Range<usize>> {
        let start = self.apply(range.start);
        let end = self.apply_before(range.end);
        (start < end).then_some(start..end)
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
    fn apply_before_keeps_offsets_before_an_insert_at_that_offset() {
        // "abcd" -> "abXYcd"
        let diff = OffsetDiff::from_edits(
            &[Edit {
                kind: EditKind::Insert,
                offset: 2,
                text: "XY".into(),
            }],
            4,
        );
        assert_eq!(diff.unchanged_before(), 2);
        assert_eq!(diff.apply(2), 4);
        assert_eq!(diff.apply_before(2), 2);
        assert_eq!(diff.apply(1), 1);
        assert_eq!(diff.apply_before(3), 5);
        // A range ending at the insert point doesn't grow; one starting
        // there moves past the insert.
        assert_eq!(diff.apply_range(0..2), Some(0..2));
        assert_eq!(diff.apply_range(2..4), Some(4..6));
    }

    #[test]
    fn apply_range_drops_ranges_that_were_deleted() {
        // "abcdef" -> "af"
        let diff = OffsetDiff::from_edits(
            &[Edit {
                kind: EditKind::Delete,
                offset: 1,
                text: "bcde".into(),
            }],
            6,
        );
        assert_eq!(diff.apply_range(2..4), None);
        assert_eq!(diff.apply_range(1..5), None);
        assert_eq!(diff.apply_range(0..6), Some(0..2));
        assert_eq!(diff.apply_range(3..6), Some(1..2));
        assert_eq!(diff.apply_before(5), 1);
        assert_eq!(diff.apply_before(1), 1);
    }

    #[test]
    fn nothing_below_the_first_edit_moves() {
        // An append, a mid-text insert, a front delete and a replacement.
        let cases: Vec<(usize, Vec<Edit>)> = vec![
            (10, vec![insert(10, "xyz")]),
            (10, vec![insert(4, "xyz")]),
            (10, vec![delete(0, "abcd")]),
            (10, vec![insert(2, "QQ"), delete(2, "cd")]),
        ];
        for (len_old, edits) in cases {
            let diff = OffsetDiff::from_edits(&edits, len_old);
            let first = diff.unchanged_before();
            assert_eq!(first, edits[0].offset);
            for offset in 0..first {
                assert_eq!(diff.apply(offset), offset);
                assert_eq!(diff.apply_before(offset), offset);
            }
            // The boundary itself is not dragged along by an insert there.
            assert_eq!(diff.apply_before(first), first);
        }
    }

    fn insert(offset: usize, text: &str) -> Edit {
        Edit {
            kind: EditKind::Insert,
            offset,
            text: text.into(),
        }
    }

    fn delete(offset: usize, text: &str) -> Edit {
        Edit {
            kind: EditKind::Delete,
            offset,
            text: text.into(),
        }
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
