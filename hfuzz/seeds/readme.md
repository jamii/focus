# Minimized fuzzer failures

Replay one with:

    cargo run --profile fuzz --bin replay hfuzz/seeds/<name>.fuzz

The `fuzz` profile, not `--release`: honggfuzz builds with debug
assertions on, and most of what it finds is an `assert_invariants` that a
plain release build compiles out - replayed with `--release` those come
back "no crash". It is optimized all the same, because the frame budget
is a claim about the editor that ships and a debug build misses it by an
order of magnitude.

These are also worth handing to honggfuzz as a starting corpus.

## Open

- `nixfmt-stack-overflow.fuzz` - 464 bytes. Saving a `.nix` file whose
  contents are a long function-application chain - `a b c d ...`, which
  is what a page of whitespace-separated tokens parses as - overflows the
  stack inside `nixfmt_rs::format::app::absorb_app`, which recurses once
  per application. About 4000 applications does it, two pages of text.
  The editor aborts: a stack overflow cannot be caught, and the daemon
  takes every open window down with it. Long lists and long attribute
  sets are fine to 40000 terms, and deeply nested parens are declined as
  a parse error rather than crashing, so it is only the chain.
- `slow-rust-tokenize.fuzz` - 8343 bytes. A tick takes ~55ms tokenizing
  138kb of Rust. `language::Tokens::new` pairs a closing bracket by
  scanning the whole stack of still-open brackets for one of its own
  kind, and a file with many unmatched openers and a stray closer of
  another kind makes that scan the whole stack every time: quadratic.
  Synthetically, `"cYu\nfv-]yOB{c"` repeated takes 2.8ms at 26kb, 10.7ms
  at 52kb, 25.9ms at 104kb and 90.8ms at 208kb - 3.5x per doubling -
  while the same unit with the `{` replaced by a letter is linear at
  80MB/s. Highlighting runs on every keystroke.

## Fixed, kept as regressions

- `slow-reload-diff-small.fuzz` - 652 bytes. Was a ~185ms tick with 5
  windows and 41kb of text.
- `slow-reload-diff.fuzz` - 829 bytes. Was a ~3.7s tick with 4 windows
  and 112kb of text.

  Both were the reload diff: a file changes on disk, the buffer reloads,
  and `buffer::replace` diffs old against new to keep cursors and undo
  history. Diffing is quadratic in the worst case and the worst case is
  text with very few distinct tokens. Fixed by `focus-core/src/diff.rs`,
  which bounds the diff with a budget counted in work;
  `focus-core/tests/diff_speed.rs` pins it.
