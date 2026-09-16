// Highlighting has to keep up with typing, which means it has to be
// linear in the size of the file: every keystroke re-reads the whole
// thing, so a rule that is merely fast on a small file is not enough.
// markdown-rs was dropped over exactly this - its parse was quadratic,
// and a 2MB document took twenty seconds a keystroke rather than forty
// milliseconds.
//
// These run in a debug build like every other test, so the bounds are
// loose: they are here to catch an accidental quadratic, or a constant
// factor that has gone an order of magnitude wrong, not to measure the
// machine. Each timing is the best of a few runs, because noise can only
// ever make one slower.

use std::path::PathBuf;
use std::time::{Duration, Instant};

mod common;

/// Eight times the text may take at most twenty-four times as long.
/// Linear is eight and quadratic is sixty-four, so this sits between them
/// with room either way: measured on a loaded machine the linear answer
/// wanders between five and ten, and anything quadratic clears sixty-four
/// by miles.
const MAX_GROWTH: f64 = 24.0;
/// A debug build of this manages around 14 MB/s on source and 6 MB/s on
/// markdown, and a release build ten times that. The floor is a smoke
/// alarm.
const MIN_MB_PER_SECOND: f64 = 1.0;

const SMALL: usize = 64 * 1024;
const LARGE: usize = 512 * 1024;
const RUNS: usize = 5;

#[test]
fn rust_highlighting_is_linear() {
    check("speed.rs", include_str!("fixtures/indent.rs"));
}

#[test]
fn python_highlighting_is_linear() {
    check("speed.py", include_str!("fixtures/indent.py"));
}

#[test]
fn shell_highlighting_is_linear() {
    check("speed.sh", include_str!("fixtures/indent.sh"));
}

#[test]
fn nix_highlighting_is_linear() {
    check("speed.nix", include_str!("fixtures/indent.nix"));
}

// Without the parser a document gets no colours, so there is nothing here
// to be slow.
#[cfg(feature = "markdown")]
#[test]
fn markdown_highlighting_is_linear() {
    check("speed.md", include_str!("fixtures/sample.md"));
}

/// Open a file of `sample` repeated to two sizes, and require that the
/// bigger one costs about what its size says it should.
fn check(name: &str, sample: &str) {
    let small = time_open(name, &repeat(sample, SMALL));
    let large = time_open(name, &repeat(sample, LARGE));

    let growth = large.as_secs_f64() / small.as_secs_f64();
    let rate = (LARGE as f64 / 1e6) / large.as_secs_f64();
    let report =
        format!("{name}: {SMALL} bytes in {small:.2?}, {LARGE} in {large:.2?} ({rate:.1} MB/s)");

    assert!(
        growth <= MAX_GROWTH,
        "{report}\n{}x the text took {growth:.1}x the time, which is not linear",
        LARGE / SMALL,
    );
    assert!(
        rate >= MIN_MB_PER_SECOND,
        "{report}\nslower than {MIN_MB_PER_SECOND} MB/s, which is an order of magnitude off",
    );
}

/// How long opening a file of this text takes, which is the tick that
/// reads it and colours it. Best of `RUNS`: the machine can only ever make
/// a run slower, so the fastest is the one that says what the work costs.
fn time_open(name: &str, text: &str) -> Duration {
    let mut best = Duration::MAX;
    for _ in 0..RUNS {
        let (mut app, mut io, _window_id) =
            common::file_app(PathBuf::from("/repo").join(name), text);
        let start = Instant::now();
        common::tick(&mut app, &mut io);
        best = best.min(start.elapsed());
    }
    best
}

fn repeat(sample: &str, target: usize) -> String {
    sample.repeat(target.div_ceil(sample.len()))
}
