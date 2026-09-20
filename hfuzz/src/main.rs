// honggfuzz target. Build & run with:
//
//   cargo install honggfuzz   # one-time, installs the cargo-hfuzz subcommand
//   cd hfuzz
//   cargo hfuzz run fuzz_hfuzz
//
// A crash found here replays with:
//
//   cargo run --release --bin replay <the .fuzz file>

use std::time::Duration;

use focus_core::fuzz::{FRAME_BUDGET, fuzz_one_with_clock};
use honggfuzz::fuzz;

// How much slower than the editor that ships this build is, near enough.
// The coverage instrumentation and the persistent-mode bookkeeping are
// not free, so a frame measured here is worth several of a real one.
// Holding this build to the real budget reports frames that are
// comfortably inside it once the instrumentation is gone: every one of
// those came back "no crash" on replay. Loose enough to let the real
// outliers through - the reload diff that found its way to four seconds
// cleared this by two orders of magnitude - and `replay` is what holds
// what turns up to the budget the editor actually promises.
const INSTRUMENTED_SLOWDOWN: u32 = 8;

// The harness times each input, tick and draw against the frame budget,
// and reading a clock is I/O, which focus-core doesn't do - so the clock
// is supplied here.
//
// It reads this thread's CPU time rather than the wall clock. honggfuzz
// runs one of these per core, so the wall clock says as much about what
// the other five are doing as about the editor: a run descheduled for
// 20ms looks exactly like a frame that took 20ms to compute, and a night
// of fuzzing fills up with crashes that replay clean. CPU time does not
// move while the thread is off the CPU, so what it measures is the work.
fn now() -> Duration {
    let mut timespec = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: writes the two integers of a timespec this call owns.
    let result = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut timespec) };
    assert_eq!(result, 0, "clock_gettime failed");
    Duration::new(timespec.tv_sec as u64, timespec.tv_nsec as u32)
}

fn main() {
    loop {
        fuzz!(|data: &[u8]| {
            fuzz_one_with_clock(data, now, FRAME_BUDGET * INSTRUMENTED_SLOWDOWN);
        });
    }
}
