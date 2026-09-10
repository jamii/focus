fn main() {
    // `fuzz_one` has input-controlled branches for all editor actions and uses
    // MockIO for the intentional effects. Keeping the input opaque ensures the
    // linker retains every branch without introducing real I/O in this binary.
    focus_core::fuzz::fuzz_one(std::hint::black_box(&[]));
}
