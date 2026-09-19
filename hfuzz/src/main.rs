// honggfuzz target. Build & run with:
//
//   cargo install honggfuzz   # one-time, installs the cargo-hfuzz subcommand
//   cd hfuzz
//   cargo hfuzz run fuzz_hfuzz
//
// For a quick crash-hunt without the honggfuzz toolchain, the workspace
// also has `cargo run --bin fuzz` which drives the same harness via the
// in-tree adaptive minimizer.

use std::sync::LazyLock;
use std::time::{Duration, Instant};

use focus_core::fuzz::fuzz_one_with_clock;
use honggfuzz::fuzz;

// The harness times each input, tick and draw against the frame budget,
// and reading a clock is I/O, which focus-core doesn't do - so the clock
// is supplied here instead. Elapsed since the first read; only the
// differences matter.
static START: LazyLock<Instant> = LazyLock::new(Instant::now);

fn now() -> Duration {
    START.elapsed()
}

fn main() {
    LazyLock::force(&START);
    loop {
        fuzz!(|data: &[u8]| {
            fuzz_one_with_clock(data, now);
        });
    }
}
