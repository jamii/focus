# Minimized fuzzer failures

Replay one with:

    cargo run --release --bin replay hfuzz/seeds/<name>.fuzz

They are also worth handing to honggfuzz as a starting corpus. Build
release: a debug build is an order of magnitude slower, and the frame
budget is a claim about the editor that ships.

Both of these used to fail the frame-budget assertion in
`focus-core/src/fuzz.rs` while inside the performance model it checks
against - at most MAX_WINDOWS windows and MAX_TEXT_BYTES of text - and
both pass now. They are kept because they are the two inputs that found
the problem, and because a diff that gives up too eagerly would still
have to get them right.

Both were the same root cause. A file changes on disk under a buffer, the
buffer reloads, and `buffer::replace` diffs the old text against the new
one to keep cursors and undo history across the reload. Diffing is
quadratic in the worst case, and the worst case is text with very few
distinct tokens: `slow-reload-diff.fuzz` diffs 20189 lines drawn from
five distinct strings against 28882 lines drawn from four, which is about
the most ambiguous input a diff can be handed. It cost a 3.7s tick.

What fixed them is `focus-core/src/diff.rs`, which bounds the diff with a
budget counted in work. `focus-core/tests/diff_speed.rs` pins that, and
is the test to look at first if these start failing again.

- `slow-reload-diff-small.fuzz` - 652 bytes. Was a ~185ms tick with 5
  windows and 41kb of text.
- `slow-reload-diff.fuzz` - 829 bytes. Was a ~3.7s tick with 4 windows
  and 112kb of text.
