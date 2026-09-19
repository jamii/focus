# Minimized fuzzer failures

Replay one with:

    cargo run --release --bin replay hfuzz/seeds/<name>.fuzz

They are also worth handing to honggfuzz as a starting corpus. Build
release: a debug build is an order of magnitude slower, and the frame
budget is a claim about the editor that ships.

Both fail the frame-budget assertion in `focus-core/src/fuzz.rs` while
inside the performance model it checks against - at most MAX_WINDOWS
windows and MAX_TEXT_BYTES of text.

Both are the same root cause. A file changes on disk under a buffer, the
buffer reloads, and `buffer::replace` diffs the old text against the new
one to keep cursors and undo history across the reload. Diffing is
quadratic in the worst case, and the worst case is text with very few
distinct tokens: the failing input diffs 20189 lines drawn from five
distinct strings against 28882 lines drawn from four, which is about the
most ambiguous input a diff can be handed.

- `slow-reload-diff-small.fuzz` - 652 bytes. One tick takes ~185ms with 5
  windows and 41kb of text. The cheaper of the two to run.
- `slow-reload-diff.fuzz` - 829 bytes. One tick takes ~3.7s with 4
  windows and 112kb of text.

Neither is fixed. They fail on purpose.

Measured while looking for a fix: the granularity and the algorithm are
both worth changing - diffing lines before words, with Myers rather than
Histogram, gives identical edits on localized changes and takes these two
to ~170ms and ~2.3s - but neither bounds the cost, because the blowup is
in the diff itself rather than in any one choice of tokens. Bounding it
needs a cap on how much work the diff is allowed to do.
