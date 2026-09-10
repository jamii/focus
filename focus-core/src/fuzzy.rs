// Lower is better: the tightest match wins, ties broken by earliest match then
// shortest name.
pub(crate) type Score = [usize; 3];

// Case-insensitive subsequence match of `pattern` against `name`.
pub(crate) fn score(name: &[u8], pattern: &[u8]) -> Option<Score> {
    // An empty pattern ties everything, leaving callers to sort by name.
    if pattern.is_empty() {
        return Some([0, 0, 0]);
    }
    let lower = |byte: u8| byte.to_ascii_lowercase();

    // Forward pass: earliest end of a subsequence match.
    let mut pattern_ix = 0;
    let mut end = 0;
    for (ix, &byte) in name.iter().enumerate() {
        if lower(byte) == lower(pattern[pattern_ix]) {
            pattern_ix += 1;
            if pattern_ix == pattern.len() {
                end = ix;
                break;
            }
        }
    }
    if pattern_ix < pattern.len() {
        return None;
    }

    // Backward pass: latest start of a match ending at `end`.
    let mut pattern_ix = pattern.len();
    let mut start = end;
    for ix in (0..=end).rev() {
        if lower(name[ix]) == lower(pattern[pattern_ix - 1]) {
            pattern_ix -= 1;
            if pattern_ix == 0 {
                start = ix;
                break;
            }
        }
    }

    Some([end - start, start, name.len()])
}
