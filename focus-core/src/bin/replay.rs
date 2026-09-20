// Replay a fuzz crash with the panic hook intact. Takes either a path to
// a raw byte file (e.g. a honggfuzz crash artifact) or a hex-encoded
// byte buffer on argv/stdin, and runs `fuzz_one`.
//
// Run with RUST_BACKTRACE=1 to see where the panic originated.

use std::io::Read;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

// A clock for the frame-budget assertion, so that a crash from it
// replays here. See `focus_core::fuzz::Clock`.
//
// The wall clock, where the honggfuzz target reads this thread's CPU
// time. The fuzzer needs CPU time because it runs one thread per core
// and the wall clock would measure its own load; replaying is one run on
// an idle machine, where the wall clock is the honest measure and is
// never below the CPU time, so anything the fuzzer found still shows up.
static START: LazyLock<Instant> = LazyLock::new(Instant::now);

fn now() -> Duration {
    START.elapsed()
}

fn hex_decode(s: &str) -> Vec<u8> {
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(s.len() % 2 == 0, "hex string must have even length");
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("invalid hex"))
        .collect()
}

fn main() {
    let bytes = if let Some(arg) = std::env::args().nth(1) {
        if std::path::Path::new(&arg).exists() {
            std::fs::read(&arg).unwrap()
        } else {
            hex_decode(&arg)
        }
    } else {
        let mut s = String::new();
        std::io::stdin().read_to_string(&mut s).unwrap();
        hex_decode(&s)
    };
    eprintln!("replaying {} bytes", bytes.len());
    LazyLock::force(&START);
    // The real budget: this is the judge, not the trigger.
    focus_core::fuzz::fuzz_one_with_clock(&bytes, now, focus_core::fuzz::FRAME_BUDGET);
    eprintln!("no crash");
}
