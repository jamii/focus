// Replay a fuzz crash with the panic hook intact. Takes either a path to
// a raw byte file (e.g. a honggfuzz crash artifact) or a hex-encoded
// byte buffer on argv/stdin, and runs `fuzz_one`.
//
// Run with RUST_BACKTRACE=1 to see where the panic originated.

use std::io::Read;

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
    focus::fuzz::fuzz_one(&bytes);
    eprintln!("no crash");
}
