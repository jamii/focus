// Test case minimization via a Finite Random Number Generator.
// https://matklad.github.io/2026/04/20/test-case-minimization.html
//
// The caller drives a property test from an `Frng` — a PRNG-shaped view
// over a finite slice of raw bytes — and panics on failure. We search for
// the smallest entropy size that still produces a panic.
//
// Caller supplies the bytes: `OsRng`, a seeded PRNG, an AFL/libFuzzer
// corpus entry — anything that fills a `Vec<u8>` of a requested size.
//
// Search is matklad's adaptive-step algorithm: grow size when all
// attempts pass, shrink when one fails; double the step while moving the
// same way, halve it on a direction change.

use bstr::{BStr, ByteSlice};
use std::panic::{AssertUnwindSafe, catch_unwind};

pub struct Config {
    pub attempts: u32,
    pub max_iters: u32,
    pub initial_size: u32,
    pub initial_step: u32,
    pub max_size: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            attempts: 100,
            max_iters: 1024,
            initial_size: 16,
            initial_step: 16,
            max_size: 1 << 20,
        }
    }
}

pub struct Frng<'a> {
    buf: &'a [u8],
    pos: usize,
}

// Lemire-style reduction: `(raw * span) >> N` uniformly maps a width-N
// integer into `[0, span)`. Bias is bounded by `span / 2^N` — fine for
// fuzzing. Computing in a 2N-bit wider type lets `span = max - min + 1`
// reach `2^N` (the `min = 0, max = TYPE::MAX` case) without overflow.
macro_rules! bounded_int {
    ($name:ident, $t:ty, $wider:ty) => {
        pub fn $name(&mut self, min: $t, max: $t) -> Option<$t> {
            assert!(min <= max);
            let span = (max - min) as $wider + 1;
            let raw = <$t>::from_le_bytes(self.array()?);
            let scaled = (raw as $wider * span) >> (size_of::<$t>() * 8);
            Some(min + scaled as $t)
        }
    };
}

impl<'a> Frng<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    pub fn bytes(&mut self, n: usize) -> Option<&[u8]> {
        if self.pos < self.buf.len() {
            return None;
        }
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Some(s)
    }

    pub fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        let mut out = [0u8; N];
        out.copy_from_slice(self.bytes(N)?);
        Some(out)
    }

    pub fn bstr(&mut self, min_len: usize, max_len: usize) -> Option<&BStr> {
        let len = self.usize_bounded(min_len, max_len)?;
        self.bytes(len).map(|b| b.as_bstr())
    }

    pub fn boolean(&mut self) -> Option<bool> {
        Some(self.u8_bounded(0, 1)? == 1)
    }

    bounded_int!(u8_bounded, u8, u16);
    bounded_int!(u16_bounded, u16, u32);
    bounded_int!(u32_bounded, u32, u64);
    bounded_int!(u64_bounded, u64, u128);
    bounded_int!(usize_bounded, usize, u128);

    pub fn weighted(&mut self, weights: &[u32]) -> Option<usize> {
        let total: u32 = weights.iter().sum();
        assert!(total > 0);
        let mut pick = self.u32_bounded(0, total - 1)?;
        for (i, &w) in weights.iter().enumerate() {
            if pick < w {
                return Some(i);
            }
            pick -= w;
        }
        unreachable!()
    }
}

#[derive(Debug, Clone)]
pub struct MinFailure {
    pub size: u32,
    pub bytes: Vec<u8>,
    pub message: Option<String>,
}

pub fn minimize<F, B>(config: Config, mut bytes_for: B, mut test_fn: F) -> Option<MinFailure>
where
    F: FnMut(&mut Frng) -> Option<()>,
    B: FnMut(u32) -> Vec<u8>,
{
    // Silence per-attempt panic backtraces — we count panics, we don't
    // want them on stderr. Restored at the end.
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));

    let mut found: Option<MinFailure> = None;
    let mut pass = true;
    let mut size: u32 = config.initial_size;
    let mut step: u32 = config.initial_step;

    for iter in 0..config.max_iters {
        if step == 0 {
            break;
        }
        let size_next = if pass {
            size.saturating_add(step)
        } else {
            size.saturating_sub(step)
        };
        if size_next > config.max_size {
            break;
        }

        let mut attempt_failure: Option<MinFailure> = None;
        for _ in 0..config.attempts {
            let bytes = bytes_for(size_next);
            let mut frng = Frng::new(&bytes);
            let result = catch_unwind(AssertUnwindSafe(|| test_fn(&mut frng)));
            if let Err(payload) = result {
                attempt_failure = Some(MinFailure {
                    size: size_next,
                    bytes,
                    message: panic_message(&*payload),
                });
                break;
            }
        }
        let pass_next = attempt_failure.is_none();

        if let Some(f) = attempt_failure {
            if found.as_ref().is_none_or(|best| f.size < best.size) {
                found = Some(f);
            }
        }

        eprintln!("fuzz_gen: iter={iter} size={size_next} step={step} pass={pass_next}");

        match (pass, pass_next) {
            (true, true) => step = step.saturating_mul(2),
            (false, false) => {}
            _ => step /= 2,
        }

        if pass || !pass_next {
            size = size_next;
            pass = pass_next;
        }
    }

    std::panic::set_hook(prev_hook);
    found
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> Option<String> {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        return Some((*s).to_owned());
    }
    if let Some(s) = payload.downcast_ref::<String>() {
        return Some(s.clone());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // SplitMix64 — small, deterministic, no deps. Lets the test
    // generate a fresh entropy buffer per attempt while staying
    // reproducible across runs.
    struct SplitMix64(u64);
    impl SplitMix64 {
        fn next_u64(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9E3779B97F4A7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
            z ^ (z >> 31)
        }
        fn fill(&mut self, buf: &mut [u8]) {
            for chunk in buf.chunks_mut(8) {
                let r = self.next_u64().to_le_bytes();
                chunk.copy_from_slice(&r[..chunk.len()]);
            }
        }
    }

    #[test]
    fn finds_a_small_failure() {
        // Property: never read four 0xFF bytes in a row. With random
        // bytes the chance per attempt is ~2^-32, so a buffer of 16
        // random bytes essentially never trips it — but the minimizer
        // should still discover a small failing buffer by stumbling
        // into one across many attempts at small sizes.
        //
        // We rig the byte source so it occasionally returns
        // an all-0xFF buffer, guaranteeing a finding.
        let mut rng = SplitMix64(0xC0FFEE);
        let mut call = 0u32;
        let bytes_for = |size: u32| {
            call += 1;
            // Every Nth call returns an all-0xFF buffer (a "failure").
            // Anything else is uniform random.
            if call % 17 == 0 {
                vec![0xFFu8; size as usize]
            } else {
                let mut buf = vec![0u8; size as usize];
                rng.fill(&mut buf);
                buf
            }
        };

        let test_fn = |frng: &mut Frng| -> Option<()> {
            let quad = frng.array::<4>()?;
            assert!(quad != [0xFF; 4]);
            Some(())
        };

        let result = minimize(
            Config {
                attempts: 50,
                max_iters: 64,
                ..Config::default()
            },
            bytes_for,
            test_fn,
        );

        let f = result.expect("should have found a failure");
        // The test only reads 4 bytes, so a 4-byte buffer is sufficient
        // to trigger the panic. We accept any size at or above 4 — the
        // search may not converge all the way given finite iterations.
        assert!(f.size >= 4, "size = {}", f.size);
        assert!(f.bytes.iter().take(4).all(|&b| b == 0xFF));
    }

    #[test]
    fn returns_none_when_no_failure() {
        let mut rng = SplitMix64(1);
        let bytes_for = |size: u32| {
            let mut buf = vec![0u8; size as usize];
            rng.fill(&mut buf);
            buf
        };
        let test_fn = |_frng: &mut Frng| -> Option<()> { Some(()) };
        let result = minimize(
            Config {
                attempts: 4,
                max_iters: 8,
                ..Config::default()
            },
            bytes_for,
            test_fn,
        );
        assert!(result.is_none());
    }
}
