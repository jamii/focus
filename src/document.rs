use std::time::SystemTime;
use std::{path::PathBuf, time::Duration};

use bstr::{BStr, BString, ByteSlice};

use crate::app::{App, IO};

pub struct Document {
    pub text: BString,
    pub newlines: Vec<usize>,
    pub queued_edits: Option<Vec<Edit>>,
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
    pub fn assert_invariants(&self) {
        assert_eq!(
            self.text.chars().filter(|c| *c == '\n').count(),
            self.newlines.len(),
        );
        for offset in &self.newlines {
            assert_eq!(self.text[*offset..].chars().next().unwrap(), '\n');
        }

        if let Some(edits) = &self.queued_edits {
            Edit::assert_invariants(edits, self.text.as_bstr());
        }
    }

    pub fn scratch() -> Document {
        Document {
            text: "".into(),
            newlines: vec![],
            queued_edits: None,
            last_modified_time: Duration::ZERO,
            last_center_offset: 0,
            source: Source::Scratch,
        }
    }

    pub fn from_file(absolute_path: PathBuf) -> Document {
        Document {
            text: "".into(),
            newlines: vec![],
            queued_edits: None,
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

    pub fn tick(&mut self, _app: &App, io: &mut dyn IO, redraw: &mut bool) {
        if let Source::File(source) = &mut self.source {
            if !(self.last_modified_time > source.last_save_time) {
                if let Some(text_new) = source.refresh_from_disk(io) {
                    source.last_save_time = io.frame_start();
                    let edits = diff_text(self.text.as_bstr(), text_new.as_bstr());
                    self.queue_edits(io, edits);
                    *redraw = true;
                }
            }
        }
    }

    pub fn queue_edits(&mut self, io: &mut dyn IO, edits: Vec<Edit>) {
        assert!(
            self.queued_edits.is_none(),
            "A set of edits is already queued"
        );
        Edit::assert_invariants(&edits, self.text.as_bstr());
        self.queued_edits = Some(edits);
        self.last_modified_time = io.frame_start();
    }

    /// Save to disk. Explicit saves create the file if missing; autosaves
    /// treat NotFound as an external deletion.
    /// No-op for Scratch sources or when not modified since last save.
    pub fn save(&mut self, io: &mut dyn IO, kind: SaveKind) {
        let absolute_path = match &self.source {
            Source::File(SourceFile {
                absolute_path,
                last_save_time,
                ..
            }) if *last_save_time > self.last_modified_time => absolute_path.clone(),
            _ => return,
        };
        let create = kind == SaveKind::Explicit;
        match io.file_write(&absolute_path, &self.text, create) {
            Ok(mtime) => {
                if let Source::File(SourceFile {
                    last_load_mtime,
                    last_save_time,
                    deleted_since_last_save,
                    ..
                }) = &mut self.source
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
                }) = &mut self.source
                {
                    *deleted_since_last_save = true;
                }
            }
            Err(err) => {
                eprintln!("error saving {}: {}", absolute_path.display(), err);
            }
        }
    }

    pub fn apply_edits(&mut self, edits: &[Edit]) -> OffsetDiff {
        Edit::assert_invariants(edits, self.text.as_bstr());

        let len_old = self.text.len();

        // TODO This can be made way more efficient, so that common cases don't have to allocate a whole new text.
        let mut text_new = BString::new(Vec::with_capacity(self.text.len()));
        let mut offset = 0;
        for edit in edits {
            text_new.extend_from_slice(&self.text[offset..edit.offset]);
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
        text_new.extend_from_slice(&self.text[offset..]);
        self.text = text_new;

        // TODO This can be made way more efficient, so that common cases don't have to iterate over the whole text.
        self.newlines.clear();
        for (char_start, _, char) in self.text.char_indices() {
            if char == '\n' {
                self.newlines.push(char_start);
            }
        }

        OffsetDiff::from_edits(edits, len_old)
    }

    pub fn grid_from_offset(&self, offset: usize) -> [usize; 2] {
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

    pub fn line_range_from_offset(&self, offset: usize) -> std::ops::Range<usize> {
        let line = self.grid_from_offset(offset)[1];
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

    pub fn char_next(&self, offset: usize) -> Option<usize> {
        if offset == self.text.len() {
            return None;
        }
        let (_, char_end, _) = self.text[offset..].char_indices().next().unwrap();
        return Some(offset + char_end);
    }

    pub fn char_prev(&self, offset: usize) -> Option<usize> {
        if offset == 0 {
            return None;
        }
        // We can't directly iter backwards through potentially invalid utf8, but we can go forwards from the start of the line.
        let line_start = self.line_range_from_offset(offset).start;
        if line_start == offset {
            // Previous character is a \n
            return Some(line_start - 1);
        }
        for (char_start, char_end, _) in self.text[line_start..].char_indices() {
            if line_start + char_end == offset {
                return Some(line_start + char_start);
            }
        }
        unreachable!()
    }
}

impl SourceFile {
    /// If the file's mtime has advanced past `last_load_mtime` and the doc is
    /// not modified, reload its contents.
    pub fn refresh_from_disk(&mut self, io: &mut dyn IO) -> Option<BString> {
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
    pub fn assert_invariants(edits: &[Edit], text: &BStr) {
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
    pub fn assert_invariants(&self) {
        assert_eq!(self.offsets_old.len() + 1, self.offsets_new.len());
        assert_eq!(self.offsets_new.len(), self.deleted.len());
        for pair in self.offsets_old.windows(2) {
            assert!(pair[0] < pair[1]);
        }
    }

    pub fn from_edits(edits: &[Edit], len_old: usize) -> Self {
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
pub fn diff_text(old: &BStr, new: &BStr) -> Vec<Edit> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(s: &str) -> Document {
        let mut doc = Document::scratch();
        doc.text = s.into();
        doc
    }

    fn ins(offset: usize, text: &str) -> Edit {
        Edit {
            kind: EditKind::Insert,
            offset,
            text: text.into(),
        }
    }

    fn del(offset: usize, text: &str) -> Edit {
        Edit {
            kind: EditKind::Delete,
            offset,
            text: text.into(),
        }
    }

    fn check(initial: &str, edits: Vec<Edit>, expected: &str) {
        let mut d = doc(initial);
        d.apply_edits(&edits);
        assert_eq!(d.text, expected);
    }

    #[test]
    fn empty_edits_is_noop() {
        check("abc", vec![], "abc");
    }

    #[test]
    fn insert_into_empty_document() {
        check("", vec![ins(0, "hello")], "hello");
    }

    #[test]
    fn insert_at_start() {
        check("bcd", vec![ins(0, "A")], "Abcd");
    }

    #[test]
    fn insert_at_end() {
        check("abc", vec![ins(3, "D")], "abcD");
    }

    #[test]
    fn insert_in_middle() {
        check("abef", vec![ins(2, "cd")], "abcdef");
    }

    #[test]
    fn multiple_sorted_inserts() {
        check("abcdef", vec![ins(2, "XX"), ins(5, "YYY")], "abXXcdeYYYf");
    }

    #[test]
    fn inserts_at_equal_positions_preserve_input_order() {
        check("ab", vec![ins(1, "X"), ins(1, "Y")], "aXYb");
    }

    #[test]
    fn all_inserts_at_position_zero() {
        check("ab", vec![ins(0, "X"), ins(0, "Y")], "XYab");
    }

    #[test]
    fn all_inserts_at_end() {
        check("ab", vec![ins(2, "X"), ins(2, "Y")], "abXY");
    }

    #[test]
    fn empty_insert_text_is_noop() {
        check("abc", vec![ins(1, "")], "abc");
    }

    #[test]
    fn mixed_empty_and_nonempty_inserts() {
        check("abc", vec![ins(0, ""), ins(1, "X"), ins(3, "")], "aXbc");
    }

    #[test]
    fn delete_entire_document() {
        check("abc", vec![del(0, "abc")], "");
    }

    #[test]
    fn delete_at_start() {
        check("abcd", vec![del(0, "a")], "bcd");
    }

    #[test]
    fn delete_at_end() {
        check("abcd", vec![del(3, "d")], "abc");
    }

    #[test]
    fn delete_in_middle() {
        check("abcdef", vec![del(2, "cd")], "abef");
    }

    #[test]
    fn multiple_sorted_deletes() {
        check("abcdef", vec![del(1, "b"), del(3, "de")], "acf");
    }

    #[test]
    fn adjacent_deletes() {
        check("abcdef", vec![del(1, "bc"), del(3, "de")], "af");
    }

    #[test]
    fn zero_length_delete_is_noop() {
        check("abc", vec![del(1, "")], "abc");
    }

    #[test]
    fn mixed_zero_and_nonzero_deletes() {
        check("abcde", vec![del(0, ""), del(1, "bc"), del(5, "")], "ade");
    }

    #[test]
    fn insert_then_delete_at_same_position() {
        check("abcd", vec![ins(1, "X"), del(1, "b")], "aXcd");
    }

    #[test]
    fn delete_then_insert_at_end_of_deletion() {
        check("abcd", vec![del(1, "b"), ins(2, "X")], "aXcd");
    }

    #[test]
    fn interleaved_inserts_and_deletes() {
        check(
            "abcdef",
            vec![ins(0, "Z"), del(1, "b"), ins(3, "Y"), del(5, "f")],
            "ZacYde",
        );
    }

    #[test]
    #[should_panic(expected = "Edit out of bounds")]
    fn panics_on_insert_out_of_bounds() {
        doc("ab").apply_edits(&[ins(3, "X")]);
    }

    #[test]
    #[should_panic(expected = "Delete out of bounds")]
    fn panics_on_delete_out_of_bounds() {
        doc("ab").apply_edits(&[del(1, "xx")]);
    }

    #[test]
    #[should_panic(expected = "Delete text doesn't match document text")]
    fn panics_on_mismatched_delete_text() {
        doc("abcd").apply_edits(&[del(1, "xx")]);
    }

    #[test]
    #[should_panic(expected = "Edits are out of order")]
    fn panics_on_unsorted_edits() {
        doc("abcd").apply_edits(&[ins(2, "X"), ins(1, "Y")]);
    }

    #[test]
    #[should_panic(expected = "Edits are out of order")]
    fn panics_on_unsorted_deletes() {
        doc("abcd").apply_edits(&[del(2, "c"), del(0, "a")]);
    }

    #[test]
    #[should_panic(expected = "Edits overlap")]
    fn panics_on_overlapping_deletes() {
        doc("abcd").apply_edits(&[del(0, "ab"), del(1, "b")]);
    }

    fn filled(s: &str) -> Document {
        let mut d = Document::scratch();
        d.apply_edits(&[ins(0, s)]);
        d
    }

    #[test]
    fn grid_from_offset_in_empty_document() {
        assert_eq!(filled("").grid_from_offset(0), [0, 0]);
    }

    #[test]
    fn grid_from_offset_on_single_line() {
        let d = filled("abc");
        assert_eq!(d.grid_from_offset(0), [0, 0]);
        assert_eq!(d.grid_from_offset(2), [2, 0]);
        assert_eq!(d.grid_from_offset(3), [3, 0]);
    }

    #[test]
    fn grid_from_offset_just_before_newline() {
        // Offset is on line 0 because the newline byte is still ahead.
        let d = filled("ab\ncd");
        assert_eq!(d.grid_from_offset(2), [2, 0]);
    }

    #[test]
    fn grid_from_offset_just_after_newline() {
        // Offset is at column 0 of line 1.
        let d = filled("ab\ncd");
        assert_eq!(d.grid_from_offset(3), [0, 1]);
    }

    #[test]
    fn grid_from_offset_mid_second_line() {
        let d = filled("ab\ncd");
        assert_eq!(d.grid_from_offset(5), [2, 1]);
    }

    #[test]
    fn grid_from_offset_across_multiple_newlines() {
        // "a\nb\nc" — newlines at byte 1 and 3.
        let d = filled("a\nb\nc");
        assert_eq!(d.grid_from_offset(0), [0, 0]);
        assert_eq!(d.grid_from_offset(2), [0, 1]);
        assert_eq!(d.grid_from_offset(4), [0, 2]);
        assert_eq!(d.grid_from_offset(5), [1, 2]);
    }

    #[test]
    fn line_range_from_offset_covers_full_line_in_unterminated_doc() {
        // No newlines anywhere; the single line should span the whole doc.
        let d = filled("abc");
        assert_eq!(d.line_range_from_offset(0), 0..3);
        assert_eq!(d.line_range_from_offset(1), 0..3);
        assert_eq!(d.line_range_from_offset(3), 0..3);
    }

    #[test]
    fn grid_from_offset_counts_chars_not_bytes() {
        // 'é' is 2 bytes but 1 column.
        let d = filled("é\nb");
        assert_eq!(d.grid_from_offset(2), [1, 0]);
        assert_eq!(d.grid_from_offset(4), [1, 1]);
    }

    fn check_diff(old: &str, new: &str) {
        let edits = diff_text(old.into(), new.into());
        let mut d = doc(old);
        d.apply_edits(&edits);
        assert_eq!(d.text, new, "edits did not transform {old:?} into {new:?}");
    }

    #[test]
    fn diff_identical() {
        assert!(diff_text(b"abc".into(), b"abc".into()).is_empty());
    }

    #[test]
    fn diff_empty_to_nonempty() {
        check_diff("", "hello\n");
    }

    #[test]
    fn diff_nonempty_to_empty() {
        check_diff("hello\n", "");
    }

    #[test]
    fn diff_replace_middle_line() {
        check_diff("a\nb\nc\n", "a\nX\nc\n");
    }

    #[test]
    fn diff_insert_lines_in_middle() {
        check_diff("a\nb\n", "a\nX\nY\nb\n");
    }

    #[test]
    fn diff_delete_lines_in_middle() {
        check_diff("a\nX\nY\nb\n", "a\nb\n");
    }

    #[test]
    fn diff_replace_one_with_two() {
        check_diff("a\nB\nc\n", "a\nX\nY\nc\n");
    }

    #[test]
    fn diff_format_like_change() {
        let old = "fn f() {\n    let x=1;\n    let y=2;\n}\n";
        let new = "fn f() {\n    let x = 1;\n    let y = 2;\n}\n";
        check_diff(old, new);
    }

    #[test]
    fn diff_no_trailing_newline() {
        check_diff("abc", "abd");
    }
}
