// honggfuzz target. Build & run with:
//
//   cargo install honggfuzz   # one-time, installs the cargo-hfuzz subcommand
//   cd hfuzz
//   cargo hfuzz run fuzz_hfuzz
//
// For a quick crash-hunt without the honggfuzz toolchain, the workspace
// also has `cargo run --bin fuzz` which drives the same harness via the
// in-tree adaptive minimizer.

use focus::fuzz::fuzz_one;
use honggfuzz::fuzz;

fn main() {
    loop {
        fuzz!(|data: &[u8]| {
            fuzz_one(data);
        });
    }
}
