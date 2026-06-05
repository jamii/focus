pub const BACKGROUND_COLOR: [u8; 4] = hsla(0.0, 0.0, 0.2, 1.0);
// pub const FADE_COLOR: [u8; 4] = hsla(0.0, 0.0, 0.2, 0.4);
// pub const STATUS_BACKGROUND_COLOR: [u8; 4] = hsla(0.0, 0.0, 0.1, 1.0);
pub const TEXT_COLOR: [u8; 4] = hsla(0.0, 0.0, 0.9, 1.0);
pub const HIGHLIGHT_COLOR: [u8; 4] = hsla(0.0, 0.0, 0.9, 0.3);
// pub const KEYWORD_COLOR: [u8; 4] = TEXT_COLOR;
// pub const COMMENT_COLOR: [u8; 4] = hsla(0.0, 0.0, 0.6, 1.0);
pub const MULTI_CURSOR_COLOR: [u8; 4] = hsla(150.0, 1.0, 0.5, 1.0);
// pub const PAREN_MATCH_COLOR: [u8; 4] = hsla(150.0, 1.0, 0.5, 0.3);

// pub const EMPHASIS_RED: [u8; 4] = hsla(0.0, 1.0, 0.5, 1.0);
// pub const EMPHASIS_ORANGE: [u8; 4] = hsla(30.0, 1.0, 0.5, 1.0);
// pub const EMPHASIS_GREEN: [u8; 4] = hsla(120.0, 1.0, 0.5, 1.0);

const fn hsla(h: f64, s: f64, l: f64, a: f64) -> [u8; 4] {
    assert!(h >= 0.0 && h < 360.0);
    assert!(s >= 0.0 && s <= 1.0);
    assert!(l >= 0.0 && l <= 1.0);
    assert!(a >= 0.0 && a <= 1.0);
    let ch = (1.0 - ((2.0 * l) - 1.0).abs()) * s;
    let h60 = h / 60.0;
    let h60_mod_2 = h60 - 2.0 * (h60 * 0.5).floor();
    let x = ch * (1.0 - (h60_mod_2 - 1.0).abs());
    let m = l - (ch / 2.0);
    let (r, g, b) = match h60 as u8 {
        0 => (ch, x, 0.0),
        1 => (x, ch, 0.0),
        2 => (0.0, ch, x),
        3 => (0.0, x, ch),
        4 => (x, 0.0, ch),
        5 => (ch, 0.0, x),
        _ => unreachable!(),
    };
    [
        (255.0 * (r + m)).round() as u8,
        (255.0 * (g + m)).round() as u8,
        (255.0 * (b + m)).round() as u8,
        (255.0 * a).round() as u8,
    ]
}

// fn ident_color(ident: &[u8]) -> [u8; 4] {
//     let mut hasher = DefaultHasher::new();
//     ident.hash(&mut hasher);
//     let hash = hasher.finish().reverse_bits();
//     hsla((hash % 360) as f64, 1.0, 0.8, 1.0)
// }

// fn paren_color(level: usize) -> [u8; 4] {
//     let mut hasher = DefaultHasher::new();
//     level.hash(&mut hasher);
//     let hash = hasher.finish().reverse_bits();
//     hsla((hash % 360) as f64, 1.0, 0.8, 1.0)
// }
