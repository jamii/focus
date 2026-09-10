// Test case minimization via a Finite Random Number Generator.
// https://matklad.github.io/2026/04/20/test-case-minimization.html
#[derive(Debug)]
pub(crate) struct Frng<'a> {
    buf: &'a [u8],
    offset: usize,
}

// Lemire-style reduction: `(raw * span) >> N` uniformly maps a width-N
// integer into `[0, span)`. Bias is bounded by `span / 2^N` — fine for
// fuzzing. Computing in a 2N-bit wider type lets `span = max - min + 1`
// reach `2^N` (the `min = 0, max = TYPE::MAX` case) without overflow.
macro_rules! bounded_int {
    ($name:ident, $t:ty, $wider:ty) => {
        pub(crate) fn $name(&mut self, min: $t, max: $t) -> Option<$t> {
            assert!(min <= max);
            let span = (max - min) as $wider + 1;
            let raw = <$t>::from_le_bytes(self.array()?);
            let scaled = (raw as $wider * span) >> (size_of::<$t>() * 8);
            Some(min + scaled as $t)
        }
    };
}

impl<'a> Frng<'a> {
    pub(crate) fn new(buf: &'a [u8]) -> Self {
        Self { buf, offset: 0 }
    }

    pub(crate) fn bytes(&mut self, n: usize) -> Option<&[u8]> {
        if self.offset + n > self.buf.len() {
            return None;
        }
        let bytes = &self.buf[self.offset..self.offset + n];
        self.offset += n;
        Some(bytes)
    }

    pub(crate) fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        let mut out = [0u8; N];
        out.copy_from_slice(self.bytes(N)?);
        Some(out)
    }

    // pub(crate) fn bstr(&mut self, min_len: usize, max_len: usize) -> Option<&BStr> {
    //     let len = self.usize_bounded(min_len, max_len)?;
    //     self.bytes(len).map(|b| b.as_bstr())
    // }

    pub(crate) fn boolean(&mut self) -> Option<bool> {
        Some(self.u8_bounded(0, 1)? == 1)
    }

    bounded_int!(u8_bounded, u8, u16);
    bounded_int!(u32_bounded, u32, u64);
    bounded_int!(usize_bounded, usize, u128);

    pub(crate) fn weighted(&mut self, weights: &[u32]) -> Option<usize> {
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
