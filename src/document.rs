use bstr::{BString, ByteSlice};

pub struct Document {
    pub text: BString,
    pub newlines: Vec<usize>,
    pub queued_edits: Option<Vec<Edit>>,
}

pub struct Edit {
    pub kind: EditKind,
    pub pos: usize,
    pub text: BString,
}

pub enum EditKind {
    Insert,
    Delete,
}

impl Document {
    pub fn new() -> Document {
        Document {
            text: "".into(),
            queued_edits: None,
            newlines: vec![],
        }
    }

    pub fn queue_edits(&mut self, edits: Vec<Edit>) {
        assert!(
            self.queued_edits.is_none(),
            "A set of edits is already queued"
        );
        self.queued_edits = Some(edits);
    }

    pub fn apply_edits(&mut self, edits: &[Edit]) {
        for edit in edits {
            assert!(edit.pos <= self.text.len(), "Edit out of bounds");
            match edit.kind {
                EditKind::Insert => {}
                EditKind::Delete => {
                    assert!(
                        edit.pos + edit.text.len() <= self.text.len(),
                        "Delete out of bounds"
                    );
                    assert_eq!(
                        edit.text,
                        self.text[edit.pos..edit.pos + edit.text.len()],
                        "Delete text doesn't match document text"
                    );
                }
            }
        }
        for pair in edits.windows(2) {
            assert!(pair[0].pos <= pair[1].pos, "Edits are out of order");
            match pair[0].kind {
                EditKind::Insert => {}
                EditKind::Delete => {
                    assert!(
                        pair[0].pos + pair[0].text.len() <= pair[1].pos,
                        "Edits overlap"
                    );
                }
            }
        }

        // TODO This can be made way more efficient, so that common cases don't have to allocate a whole new text.
        let mut text_new = BString::new(Vec::with_capacity(self.text.len()));
        let mut pos = 0;
        for edit in edits {
            text_new.extend_from_slice(&self.text[pos..edit.pos]);
            pos = edit.pos;
            match edit.kind {
                EditKind::Insert => {
                    text_new.extend_from_slice(&edit.text);
                }
                EditKind::Delete => {
                    pos += edit.text.len();
                }
            }
        }
        text_new.extend_from_slice(&self.text[pos..]);
        self.text = text_new;

        // TODO This can be made way more efficient, so that common cases don't have to iterate over the whole text.
        self.newlines.clear();
        for (char_start, _, char) in self.text.char_indices() {
            if char == '\n' {
                self.newlines.push(char_start);
            }
        }
    }

    pub fn grid_from_pos(&self, pos: usize) -> [usize; 2] {
        // TODO binary search
        for (line, newline_pos) in self.newlines.iter().enumerate().rev() {
            if *newline_pos < pos {
                let col = self.text[*newline_pos + 1..pos].chars().count();
                return [col, line + 1];
            }
        }
        let col = self.text[0..pos].chars().count();
        [col, 0]
    }

    pub fn line_range_from_pos(&self, pos: usize) -> std::ops::Range<usize> {
        let line = self.grid_from_pos(pos)[1];
        if line == 0 {
            0..{
                if self.newlines.len() == 0 {
                    0
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

    pub fn char_next(&self, pos: usize) -> Option<usize> {
        if pos == self.text.len() {
            return None;
        }
        let (_, char_end, _) = self.text[pos..].char_indices().next().unwrap();
        return Some(pos + char_end);
    }

    pub fn char_prev(&self, pos: usize) -> Option<usize> {
        if pos == 0 {
            return None;
        }
        // We can't directly iter backwards through potentially invalid utf8, but we can go forwards from the start of the line.
        let line_start = self.line_range_from_pos(pos).start;
        if line_start == pos {
            // Previous character is a \n
            return Some(line_start - 1);
        }
        for (char_start, char_end, _) in self.text[line_start..].char_indices() {
            if line_start + char_end == pos {
                return Some(line_start + char_start);
            }
        }
        unreachable!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(s: &str) -> Document {
        let mut doc = Document::new();
        doc.text = s.into();
        doc
    }

    fn ins(pos: usize, text: &str) -> Edit {
        Edit {
            kind: EditKind::Insert,
            pos,
            text: text.into(),
        }
    }

    fn del(pos: usize, text: &str) -> Edit {
        Edit {
            kind: EditKind::Delete,
            pos,
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
}
