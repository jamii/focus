// Reloading a file that changed underneath a buffer diffs the old text
// against the new one, so that cursors and undo history survive. Diffing
// is quadratic in the worst case, and the worst case is not exotic: a
// file whose lines are nearly all the same as each other - a log, a csv,
// anything generated - gives the search nothing to anchor on and every
// token something to match. The diff is bounded by a work budget rather
// than left to take as long as it takes, and this is what pins that.
//
// These run in a debug build like every other test, so the bounds are
// loose: they are here to catch the budget being removed or raised by an
// order of magnitude, not to measure the machine. Each timing is the best
// of a few runs, because noise can only ever make one slower.

use std::path::PathBuf;
use std::time::{Duration, Instant};

mod common;

/// A reload may not take longer than this. A debug build runs the diff
/// about ten times slower than the release build that ships, where the
/// budget buys about 6ms, so this is that with room for the machine.
const MAX_RELOAD: Duration = Duration::from_millis(400);

const RUNS: usize = 3;

/// Two texts of repeated lines with nothing in common: every token
/// matches something, no token identifies anything, and the search never
/// converges. Without a budget this takes seconds.
#[test]
fn reloading_over_unrelated_repetitive_text_is_bounded() {
    let before = "plain line of text with words in it\n".repeat(5_600);
    let after = "quite different words appear on every line\n".repeat(4_700);
    check("repetitive", &before, &after);
}

/// The shape the fuzzer finds: a handful of distinct short lines
/// repeated thousands of times on each side, sharing only the blank line
/// between them. Every blank matches every other blank, so the search
/// has an enormous number of equally good paths and no reason to prefer
/// any of them.
#[test]
fn reloading_over_a_tiny_line_alphabet_is_bounded() {
    let before = "c8y\nT1[z]F|w<\n~Nv@[L\n\n".repeat(9_000);
    let after = "\n\nzNQX5\n\nz7Enp\n".repeat(9_000);
    check("tiny alphabet", &before, &after);
}

/// The case the budget must not spoil: a big file, a small edit. This
/// one is nowhere near the budget, and the diff stays exact.
#[test]
fn reloading_after_a_small_edit_is_fast() {
    let before = "fn foo(x: u32) -> u32 {\n    let y = x + 1;\n    y * 2\n}\n\n".repeat(3_600);
    let after = before.replacen("x + 1", "x + 41", 1);
    let elapsed = check("small edit", &before, &after);
    assert!(
        elapsed < MAX_RELOAD / 4,
        "a one-token edit in a {}kb file took {elapsed:.2?}",
        before.len() / 1000,
    );
}

/// Load a file, change it on disk, and time the tick that notices.
fn check(name: &str, before: &str, after: &str) -> Duration {
    let mut best = Duration::MAX;
    for _ in 0..RUNS {
        let path = PathBuf::from("/repo/reload.txt");
        let (mut app, mut io, _window_id) = common::file_app(path.clone(), before);
        common::tick(&mut app, &mut io);

        let mtime = io.files.get(&path).map(|(_, mtime)| *mtime).unwrap() + Duration::from_secs(1);
        io.system_time = mtime;
        io.files
            .insert(path.clone(), (after.as_bytes().to_vec(), mtime));

        let start = Instant::now();
        common::tick(&mut app, &mut io);
        best = best.min(start.elapsed());
    }

    assert!(
        best <= MAX_RELOAD,
        "{name}: reloading {}kb over {}kb took {best:.2?}, over the {MAX_RELOAD:.2?} bound",
        after.len() / 1000,
        before.len() / 1000,
    );
    best
}
