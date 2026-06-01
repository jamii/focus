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

#[derive(Debug)]
pub struct Frng<'a> {
    buf: &'a [u8],
    offset: usize,
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
        Self { buf, offset: 0 }
    }

    pub fn bytes(&mut self, n: usize) -> Option<&[u8]> {
        if self.offset + n > self.buf.len() {
            return None;
        }
        let bytes = &self.buf[self.offset..self.offset + n];
        self.offset += n;
        Some(bytes)
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
