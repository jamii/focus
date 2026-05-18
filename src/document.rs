use bstr::{BString, ByteSlice};

use crate::style::TEXT_COLOR;
use crate::{app::App, text::Drawing};

pub struct Document {
    pub text: BString,
}

pub struct Insert {
    pub pos: usize,
    pub text: BString,
}

pub struct Delete {
    pub pos: usize,
    pub len: usize,
}

impl Document {
    pub fn new() -> Document {
        Document { text: "".into() }
    }

    pub fn draw(&self, app: &App, drawing: &mut Drawing) {
        drawing.draw_text(&app.atlas, self.text.as_bstr(), 0.0, 0.0, TEXT_COLOR);
    }

    pub fn replace(&mut self, text: BString) {
        self.text = text;
    }

    pub fn insert(&mut self, inserts: Vec<Insert>) {
        for insert in inserts.iter() {
            assert!(insert.pos <= self.text.len(), "Insert out of bounds");
        }
        for pair in inserts.windows(2) {
            assert!(pair[0].pos <= pair[1].pos, "Inserts should be sorted");
        }
        let len_old = self.text.len();
        let len_added = inserts.iter().map(|i| i.text.len()).sum::<usize>();
        let len_new = len_old + len_added;
        self.text.resize(len_new, 0);
        let mut gap = len_old..len_new;
        for insert in inserts.iter().rev() {
            let len = gap.start - insert.pos;
            self.text.copy_within(insert.pos..gap.start, gap.end - len);
            gap.start -= len;
            gap.end -= len;
            self.text[gap.end - insert.text.len()..gap.end].copy_from_slice(&insert.text);
            gap.end -= insert.text.len();
        }
        assert!(gap.start == gap.end);
    }

    pub fn delete(&mut self, deletes: Vec<Delete>) {
        for delete in deletes.iter() {
            assert!(
                delete.pos + delete.len <= self.text.len(),
                "Delete out of bounds"
            );
        }
        for pair in deletes.windows(2) {
            assert!(
                pair[0].pos + pair[0].len <= pair[1].pos,
                "Deletes should be sorted and non-overlapping"
            );
        }
        let len_old = self.text.len();
        let len_removed = deletes.iter().map(|d| d.len).sum::<usize>();
        let len_new = len_old - len_removed;
        let mut gap = 0..0;
        for delete in deletes.iter() {
            let len = delete.pos - gap.end;
            self.text.copy_within(gap.end..delete.pos, gap.start);
            gap.start += len;
            gap.end = delete.pos + delete.len;
        }
        let len = len_old - gap.end;
        self.text.copy_within(gap.end..len_old, gap.start);
        gap.start += len;
        assert!(gap.start == len_new);
        self.text.truncate(len_new);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(s: &str) -> Document {
        Document { text: s.into() }
    }

    fn ins(pos: usize, text: &str) -> Insert {
        Insert {
            pos,
            text: text.into(),
        }
    }

    fn del(pos: usize, len: usize) -> Delete {
        Delete { pos, len }
    }

    fn check(initial: &str, inserts: Vec<Insert>, expected: &str) {
        let mut d = doc(initial);
        d.insert(inserts);
        assert_eq!(d.text, expected);
    }

    fn check_del(initial: &str, deletes: Vec<Delete>, expected: &str) {
        let mut d = doc(initial);
        d.delete(deletes);
        assert_eq!(d.text, expected);
    }

    #[test]
    fn empty_inserts_is_noop() {
        check("abc", vec![], "abc");
    }

    #[test]
    fn insert_into_empty_document() {
        check("", vec![ins(0, "hello")], "hello");
    }

    #[test]
    fn single_at_start() {
        check("bcd", vec![ins(0, "A")], "Abcd");
    }

    #[test]
    fn single_at_end() {
        check("abc", vec![ins(3, "D")], "abcD");
    }

    #[test]
    fn single_in_middle() {
        check("abef", vec![ins(2, "cd")], "abcdef");
    }

    #[test]
    fn multiple_sorted_inserts() {
        check("abcdef", vec![ins(2, "XX"), ins(5, "YYY")], "abXXcdeYYYf");
    }

    #[test]
    fn equal_positions_preserve_input_order() {
        check("ab", vec![ins(1, "X"), ins(1, "Y")], "aXYb");
    }

    #[test]
    fn all_at_position_zero() {
        check("ab", vec![ins(0, "X"), ins(0, "Y")], "XYab");
    }

    #[test]
    fn all_at_end() {
        check("ab", vec![ins(2, "X"), ins(2, "Y")], "abXY");
    }

    #[test]
    fn empty_insert_text_is_noop() {
        check("abc", vec![ins(1, "")], "abc");
    }

    #[test]
    fn mixed_empty_and_nonempty() {
        check("abc", vec![ins(0, ""), ins(1, "X"), ins(3, "")], "aXbc");
    }

    #[test]
    #[should_panic(expected = "Insert out of bounds")]
    fn panics_on_out_of_bounds() {
        doc("ab").insert(vec![ins(3, "X")]);
    }

    #[test]
    #[should_panic(expected = "Inserts should be sorted")]
    fn panics_on_unsorted() {
        doc("abc").insert(vec![ins(2, "X"), ins(1, "Y")]);
    }

    #[test]
    fn delete_empty_is_noop() {
        check_del("abc", vec![], "abc");
    }

    #[test]
    fn delete_entire_document() {
        check_del("abc", vec![del(0, 3)], "");
    }

    #[test]
    fn delete_single_at_start() {
        check_del("abcd", vec![del(0, 1)], "bcd");
    }

    #[test]
    fn delete_single_at_end() {
        check_del("abcd", vec![del(3, 1)], "abc");
    }

    #[test]
    fn delete_single_in_middle() {
        check_del("abcdef", vec![del(2, 2)], "abef");
    }

    #[test]
    fn delete_multiple_sorted() {
        check_del("abcdef", vec![del(1, 1), del(3, 2)], "acf");
    }

    #[test]
    fn delete_adjacent_ranges() {
        // pair[0].pos + pair[0].len == pair[1].pos — touching but not overlapping.
        check_del("abcdef", vec![del(1, 2), del(3, 2)], "af");
    }

    #[test]
    fn delete_zero_length_is_noop() {
        check_del("abc", vec![del(1, 0)], "abc");
    }

    #[test]
    fn delete_mixed_zero_and_nonzero() {
        check_del("abcde", vec![del(0, 0), del(1, 2), del(5, 0)], "ade");
    }

    #[test]
    #[should_panic(expected = "Delete out of bounds")]
    fn delete_panics_on_out_of_bounds() {
        doc("ab").delete(vec![del(1, 2)]);
    }

    #[test]
    #[should_panic(expected = "Deletes should be sorted and non-overlapping")]
    fn delete_panics_on_unsorted() {
        doc("abcd").delete(vec![del(2, 1), del(0, 1)]);
    }

    #[test]
    #[should_panic(expected = "Deletes should be sorted and non-overlapping")]
    fn delete_panics_on_overlapping() {
        doc("abcd").delete(vec![del(0, 2), del(1, 1)]);
    }
}
