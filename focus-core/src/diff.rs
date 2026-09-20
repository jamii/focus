// Myers' diff, with a budget counted in work rather than in time.
//
// The algorithm is Eugene Myers' O((N+M)D) linear-space divide and
// conquer diff (http://www.xmailserver.org/diff2.pdf). It is copied from
// the `similar` crate - src/algorithms/myers.rs in 3.1.1, which took it
// in turn from Brandon Williams' implementation - with two changes:
//
//   * `similar` bounds a hard input with a wall-clock deadline, which is
//     no good here twice over. Reading a clock is ambient I/O, which
//     focus-core does not do (see tests/reachable_io.rs). Worse, it makes
//     the answer depend on how busy the machine is: the same file
//     reloading would leave a different undo history on a loaded machine
//     than on an idle one, and a fuzz crash would not replay. This counts
//     the work instead, which is a property of the two inputs alone.
//
//   * the `DiffHook` trait and the generic `Index`/`PartialEq` bounds are
//     gone. This diffs one slice of tokens against another and returns
//     the ops, which is all the one caller wants.
//
// The heuristics `similar` layers on top of the algorithm - the
// front-anchor peel for unbalanced shifts, and the exact small-side
// fallback - are not copied, and neither is its `Compact` pass, which
// slides hunk boundaries around to read better in a unified diff. None of
// them change what a diff means; if one turns out to be worth its weight
// it can be copied too.

use bstr::{BStr, ByteSlice};

use std::ops::{Index, IndexMut, Range};

/// How much work `diff` may do before it gives up and calls the rest of
/// the text changed.
///
/// The unit is one step of the middle-snake search: either extending a
/// path by one diagonal, or comparing one pair of tokens while running
/// along a snake. Scanning off a common prefix or suffix is not counted -
/// that work is linear in the input and it is what makes the ordinary
/// case (a big file, a small edit) cheap, so there is no reason to ration
/// it.
///
/// Sized from measurement rather than derived: see tests/diff_speed.rs,
/// which pins both the budget's cost and what it buys.
pub(crate) const DEFAULT_BUDGET: usize = 1_000_000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum OpKind {
    Equal,
    Delete,
    Insert,
    Replace,
}

/// One run of the diff, as a range of `old` tokens and a range of `new`
/// ones. `Delete` has an empty `new` range and `Insert` an empty `old`
/// one; both say where in the other side the change sits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Op {
    pub(crate) kind: OpKind,
    pub(crate) old: Range<usize>,
    pub(crate) new: Range<usize>,
}

/// Split `text` into the tokens a diff compares: maximal runs of
/// whitespace, and maximal runs of everything else.
///
/// Word granularity, which is the granularity the edits want - a
/// one-word change should move one word, not a whole line. Splitting on
/// characters rather than bytes keeps a multi-byte character whole, and
/// bytes that are not a character at all travel with the run they are in
/// rather than breaking it.
///
/// The tokens tile the text, so a token index can be turned into a byte
/// offset by summing the lengths in front of it.
pub(crate) fn tokenize_words(text: &BStr) -> Vec<&[u8]> {
    let mut tokens = Vec::new();
    let mut chars = text.char_indices().peekable();
    while let Some((start, mut end, char)) = chars.next() {
        let whitespace = char.is_whitespace();
        while let Some(&(_, next_end, next)) = chars.peek() {
            if next.is_whitespace() != whitespace {
                break;
            }
            chars.next();
            end = next_end;
        }
        tokens.push(&text.as_bytes()[start..end]);
    }
    tokens
}

/// The ops that turn `old` into `new`.
///
/// Gives up gracefully: when `budget` runs out the regions it has not
/// looked at yet come back as `Replace`, which is what a diff that has
/// found nothing in common says anyway.
pub(crate) fn diff(old: &[&[u8]], new: &[&[u8]], budget: usize) -> Vec<Op> {
    let mut ops = Vec::new();
    let mut budget = Budget(budget);
    let max_d = max_d(old.len(), new.len());
    let mut vf = V::new(max_d);
    let mut vb = V::new(max_d);
    conquer(
        &mut ops,
        old,
        0..old.len(),
        new,
        0..new.len(),
        &mut vf,
        &mut vb,
        &mut budget,
    );
    coalesce_replaces(ops)
}

struct Budget(usize);

impl Budget {
    fn spend(&mut self, work: usize) {
        self.0 = self.0.saturating_sub(work);
    }

    fn is_spent(&self) -> bool {
        self.0 == 0
    }
}

// A D-path is a path which starts at (0,0) that has exactly D non-diagonal
// edges. All D-paths consist of a (D - 1)-path followed by a non-diagonal edge
// and then a possibly empty sequence of diagonal edges called a snake.

/// `V` contains the endpoints of the furthest reaching `D-paths`. For each
/// recorded endpoint `(x,y)` in diagonal `k`, we only need to retain `x` because
/// `y` can be computed from `x - k`. In other words, `V` is an array of integers
/// where `V[k]` contains the row index of the endpoint of the furthest reaching
/// path in diagonal `k`.
///
/// We can't use a traditional Vec to represent `V` since we use `k` as an index
/// and it can take on negative values. So instead `V` is represented as a
/// light-weight wrapper around a Vec plus an `offset` which is the maximum value
/// `k` can take on in order to map negative `k`'s back to a value >= 0.
struct V {
    offset: isize,
    v: Vec<usize>,
}

impl V {
    fn new(max_d: usize) -> Self {
        V {
            offset: max_d as isize,
            v: vec![0; 2 * max_d],
        }
    }

    fn len(&self) -> usize {
        self.v.len()
    }
}

impl Index<isize> for V {
    type Output = usize;

    fn index(&self, index: isize) -> &Self::Output {
        &self.v[(index + self.offset) as usize]
    }
}

impl IndexMut<isize> for V {
    fn index_mut(&mut self, index: isize) -> &mut Self::Output {
        &mut self.v[(index + self.offset) as usize]
    }
}

fn max_d(len1: usize, len2: usize) -> usize {
    (len1 + len2).div_ceil(2) + 1
}

fn common_prefix_len(
    old: &[&[u8]],
    old_range: Range<usize>,
    new: &[&[u8]],
    new_range: Range<usize>,
) -> usize {
    let max_len = old_range.len().min(new_range.len());
    let mut matched = 0;
    while matched < max_len && new[new_range.start + matched] == old[old_range.start + matched] {
        matched += 1;
    }
    matched
}

fn common_suffix_len(
    old: &[&[u8]],
    old_range: Range<usize>,
    new: &[&[u8]],
    new_range: Range<usize>,
) -> usize {
    let max_len = old_range.len().min(new_range.len());
    let mut matched = 0;
    while matched < max_len && new[new_range.end - 1 - matched] == old[old_range.end - 1 - matched]
    {
        matched += 1;
    }
    matched
}

fn split_at(range: Range<usize>, at: usize) -> (Range<usize>, Range<usize>) {
    (range.start..at, at..range.end)
}

/// The divide part of a divide-and-conquer strategy. A D-path has D+1 snakes
/// some of which may be empty. The divide step requires finding the ceil(D/2) +
/// 1 or middle snake of an optimal D-path. The idea for doing so is to
/// simultaneously run the basic algorithm in both the forward and reverse
/// directions until furthest reaching forward and reverse paths starting at
/// opposing corners 'overlap'.
///
/// Returns `None` when the budget ran out before the paths met.
fn find_middle_snake(
    old: &[&[u8]],
    old_range: Range<usize>,
    new: &[&[u8]],
    new_range: Range<usize>,
    vf: &mut V,
    vb: &mut V,
    budget: &mut Budget,
) -> Option<(usize, usize)> {
    let n = old_range.len();
    let m = new_range.len();

    // By Lemma 1 in the paper, the optimal edit script length is odd or even as
    // `delta` is odd or even.
    let delta = n as isize - m as isize;
    let odd = delta & 1 == 1;

    // The initial point at (0, -1)
    vf[1] = 0;
    // The initial point at (N, M+1)
    vb[1] = 0;

    // We only need to explore ceil(D/2) + 1
    let d_max = max_d(n, m);
    assert!(vf.len() >= d_max);
    assert!(vb.len() >= d_max);

    for d in 0..d_max as isize {
        // Have we been at this for too long?
        if budget.is_spent() {
            break;
        }

        // Forward path
        for k in (-d..=d).rev().step_by(2) {
            budget.spend(1);
            let mut x = if k == -d || (k != d && vf[k - 1] < vf[k + 1]) {
                vf[k + 1]
            } else {
                vf[k - 1] + 1
            };
            let y = (x as isize - k) as usize;

            // The coordinate of the start of a snake
            let (x0, y0) = (x, y);
            //  While these sequences are identical, keep moving through the
            //  graph with no cost
            if x < n && y < m {
                let advance = common_prefix_len(
                    old,
                    old_range.start + x..old_range.end,
                    new,
                    new_range.start + y..new_range.end,
                );
                budget.spend(advance);
                x += advance;
            }

            // This is the new best x value
            vf[k] = x;

            // Only check for connections from the forward search when N - M is
            // odd and when there is a reciprocal k line coming from the other
            // direction.
            if odd && (k - delta).abs() <= (d - 1) && vf[k] + vb[-(k - delta)] >= n {
                // Return the snake
                return Some((x0 + old_range.start, y0 + new_range.start));
            }
        }

        // Backward path
        for k in (-d..=d).rev().step_by(2) {
            budget.spend(1);
            let mut x = if k == -d || (k != d && vb[k - 1] < vb[k + 1]) {
                vb[k + 1]
            } else {
                vb[k - 1] + 1
            };
            let mut y = (x as isize - k) as usize;

            // The coordinate of the start of a snake
            if x < n && y < m {
                let advance = common_suffix_len(
                    old,
                    old_range.start..old_range.start + n - x,
                    new,
                    new_range.start..new_range.start + m - y,
                );
                budget.spend(advance);
                x += advance;
                y += advance;
            }

            // This is the new best x value
            vb[k] = x;

            if !odd && (k - delta).abs() <= d && vb[k] + vf[-(k - delta)] >= n {
                // Return the snake
                return Some((n - x + old_range.start, m - y + new_range.start));
            }
        }
    }

    // Out of budget.
    None
}

#[allow(clippy::too_many_arguments)]
fn conquer(
    ops: &mut Vec<Op>,
    old: &[&[u8]],
    mut old_range: Range<usize>,
    new: &[&[u8]],
    mut new_range: Range<usize>,
    vf: &mut V,
    vb: &mut V,
    budget: &mut Budget,
) {
    // Check for common prefix
    let prefix_len = common_prefix_len(old, old_range.clone(), new, new_range.clone());
    if prefix_len > 0 {
        push(
            ops,
            OpKind::Equal,
            old_range.start..old_range.start + prefix_len,
            new_range.start..new_range.start + prefix_len,
        );
    }
    old_range.start += prefix_len;
    new_range.start += prefix_len;

    // Check for common suffix
    let suffix_len = common_suffix_len(old, old_range.clone(), new, new_range.clone());
    let suffix = (old_range.end - suffix_len, new_range.end - suffix_len);
    old_range.end -= suffix_len;
    new_range.end -= suffix_len;

    if old_range.is_empty() && new_range.is_empty() {
        // Do nothing
    } else if new_range.is_empty() {
        push(
            ops,
            OpKind::Delete,
            old_range.clone(),
            new_range.start..new_range.start,
        );
    } else if old_range.is_empty() {
        push(
            ops,
            OpKind::Insert,
            old_range.start..old_range.start,
            new_range.clone(),
        );
    } else if let Some((x_start, y_start)) = find_middle_snake(
        old,
        old_range.clone(),
        new,
        new_range.clone(),
        vf,
        vb,
        budget,
    ) {
        let (old_a, old_b) = split_at(old_range, x_start);
        let (new_a, new_b) = split_at(new_range, y_start);
        conquer(ops, old, old_a, new, new_a, vf, vb, budget);
        conquer(ops, old, old_b, new, new_b, vf, vb, budget);
    } else {
        // Out of budget. What is left is reported as wholly changed,
        // which is what the search would have concluded had it found
        // nothing in common.
        push(ops, OpKind::Replace, old_range, new_range);
    }

    if suffix_len > 0 {
        push(
            ops,
            OpKind::Equal,
            suffix.0..suffix.0 + suffix_len,
            suffix.1..suffix.1 + suffix_len,
        );
    }
}

/// Append an op, extending the last one when this continues it. `conquer`
/// emits in order, so only the last op can ever be the one to extend.
fn push(ops: &mut Vec<Op>, kind: OpKind, old: Range<usize>, new: Range<usize>) {
    if let Some(last) = ops.last_mut()
        && last.kind == kind
        && last.old.end == old.start
        && last.new.end == new.start
    {
        last.old.end = old.end;
        last.new.end = new.end;
        return;
    }
    ops.push(Op { kind, old, new });
}

/// A delete with an insert against it is a replacement. They arrive as
/// two ops because the algorithm finds them separately.
fn coalesce_replaces(ops: Vec<Op>) -> Vec<Op> {
    let mut out: Vec<Op> = Vec::with_capacity(ops.len());
    for op in ops {
        let merged = match out.last_mut() {
            Some(last) if last.old.end == op.old.start && last.new.end == op.new.start => {
                match (last.kind, op.kind) {
                    (OpKind::Delete, OpKind::Insert) | (OpKind::Insert, OpKind::Delete) => {
                        last.kind = OpKind::Replace;
                        last.old.end = op.old.end;
                        last.new.end = op.new.end;
                        true
                    }
                    _ => false,
                }
            }
            _ => false,
        };
        if !merged {
            out.push(op);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(text: &str) -> Vec<&[u8]> {
        text.split_inclusive(' ').map(|s| s.as_bytes()).collect()
    }

    /// Walk the ops and rebuild `new` out of them, which is the only
    /// thing a diff has to get right.
    fn apply(old: &[&[u8]], new: &[&[u8]], ops: &[Op]) -> String {
        let mut out = Vec::new();
        let mut old_at = 0;
        let mut new_at = 0;
        for op in ops {
            assert_eq!(op.old.start, old_at, "ops must tile old: {ops:?}");
            assert_eq!(op.new.start, new_at, "ops must tile new: {ops:?}");
            match op.kind {
                OpKind::Equal => {
                    assert_eq!(op.old.len(), op.new.len());
                    for token in &old[op.old.clone()] {
                        out.extend_from_slice(token);
                    }
                }
                OpKind::Delete => assert!(op.new.is_empty()),
                OpKind::Insert | OpKind::Replace => {
                    for token in &new[op.new.clone()] {
                        out.extend_from_slice(token);
                    }
                }
            }
            old_at = op.old.end;
            new_at = op.new.end;
        }
        assert_eq!(old_at, old.len());
        assert_eq!(new_at, new.len());
        String::from_utf8(out).unwrap()
    }

    fn roundtrip(old: &str, new: &str, budget: usize) -> Vec<Op> {
        let old_tokens = tokens(old);
        let new_tokens = tokens(new);
        let ops = diff(&old_tokens, &new_tokens, budget);
        assert_eq!(apply(&old_tokens, &new_tokens, &ops), new);
        ops
    }

    #[test]
    fn tokenize_words_tiles_the_text() {
        let cases: Vec<Vec<u8>> = vec![
            b"".to_vec(),
            b"a".to_vec(),
            b" ".to_vec(),
            b"  leading and  trailing   ".to_vec(),
            b"tabs\tand\nnewlines\r\n mixed".to_vec(),
            "caf\u{e9} r\u{e9}sum\u{e9} \u{732b} \u{1f980}"
                .as_bytes()
                .to_vec(),
            // Bytes that are not characters at all still have to come
            // back out in one piece.
            vec![0xff, 0xfe, b' ', 0x80, b'a', 0xc3, 0x28],
            (0u8..=255).collect(),
        ];
        for case in &cases {
            let tokens = tokenize_words(case.as_bstr());
            assert_eq!(tokens.concat(), *case, "on {:?}", case.as_bstr());
            assert!(tokens.iter().all(|token| !token.is_empty()));
        }
    }

    #[test]
    fn tokenize_words_splits_on_whitespace_runs() {
        let tokens = tokenize_words(b"the  quick\tfox".as_bstr());
        assert_eq!(tokens, [&b"the"[..], b"  ", b"quick", b"\t", b"fox"]);
    }

    #[test]
    fn identical_text_is_one_equal() {
        let ops = roundtrip("a b c", "a b c", DEFAULT_BUDGET);
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].kind, OpKind::Equal);
    }

    #[test]
    fn a_changed_run_is_one_replace() {
        let ops = roundtrip("the quick brown fox", "the slow fox", DEFAULT_BUDGET);
        let kinds: Vec<_> = ops.iter().map(|op| op.kind).collect();
        assert_eq!(
            kinds,
            [OpKind::Equal, OpKind::Replace, OpKind::Equal],
            "{ops:?}"
        );
    }

    #[test]
    fn insert_and_delete_at_the_ends() {
        let ops = roundtrip("b c", "a b c", DEFAULT_BUDGET);
        assert_eq!(ops[0].kind, OpKind::Insert);
        let ops = roundtrip("a b c", "b c", DEFAULT_BUDGET);
        assert_eq!(ops[0].kind, OpKind::Delete);
    }

    #[test]
    fn empty_sides() {
        assert!(diff(&[], &[], DEFAULT_BUDGET).is_empty());
        roundtrip("", "a b", DEFAULT_BUDGET);
        roundtrip("a b", "", DEFAULT_BUDGET);
    }

    /// The point of the budget: it bounds the work, and what it gives up
    /// is precision rather than correctness.
    #[test]
    fn a_spent_budget_still_produces_a_usable_diff() {
        let old = "one two three four five six seven eight nine ten ".repeat(100);
        let new = "ten nine eight seven six five four three two one ".repeat(100);
        for budget in [0, 1, 10, 1_000, DEFAULT_BUDGET] {
            let ops = roundtrip(&old, &new, budget);
            assert!(!ops.is_empty());
        }
    }

    /// A budget of zero is the coarsest answer there is, and it is still
    /// a correct one.
    #[test]
    fn a_zero_budget_replaces_everything() {
        let ops = roundtrip("a b c", "x y z", 0);
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].kind, OpKind::Replace);
    }

    /// Same inputs, same answer, whatever the machine is doing - which is
    /// the whole reason this counts work rather than watching a clock.
    #[test]
    fn the_same_inputs_give_the_same_ops() {
        let old = "alpha beta gamma delta epsilon ".repeat(50);
        let new = "alpha gamma beta epsilon delta ".repeat(50);
        let old_tokens = tokens(&old);
        let new_tokens = tokens(&new);
        let first = diff(&old_tokens, &new_tokens, 5_000);
        for _ in 0..8 {
            assert_eq!(diff(&old_tokens, &new_tokens, 5_000), first);
        }
    }
}
