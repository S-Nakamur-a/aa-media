/// Kanji character lookup by visual density and edge direction.
///
/// Each cell of the image is mapped to a kanji whose stroke density matches
/// the luminance and whose visual texture matches the dominant edge direction.

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Uniform,    // No strong edge — use symmetric/boxy characters
    Horizontal, // — edge
    Vertical,   // | edge
    DiagRight,  // / edge (lower-left to upper-right)
    DiagLeft,   // \ edge (upper-left to lower-right)
}

/// Pixel block size per kanji cell (each kanji analyses CELL×CELL source pixels).
pub const CELL: usize = 4;

/// Gradient magnitude threshold below which the cell is considered "uniform".
const MAG_THRESHOLD: f32 = 50.0;

/// Classify edge direction from averaged gradient components (gx, gy).
pub fn classify(gx: f32, gy: f32) -> Direction {
    let mag = (gx * gx + gy * gy).sqrt();
    if mag < MAG_THRESHOLD {
        return Direction::Uniform;
    }

    let ax = gx.abs();
    let ay = gy.abs();

    if ax > ay * 2.0 {
        // Gradient mostly horizontal → edge runs vertically
        Direction::Vertical
    } else if ay > ax * 2.0 {
        // Gradient mostly vertical → edge runs horizontally
        Direction::Horizontal
    } else if (gx > 0.0) == (gy > 0.0) {
        // Gradient ↘ or ↖ → edge perpendicular = /
        Direction::DiagRight
    } else {
        // Gradient ↗ or ↙ → edge perpendicular = \
        Direction::DiagLeft
    }
}

/// Find the best-matching kanji for given density (0.0–1.0) and direction.
/// Density 0.0 = darkest (space), 1.0 = brightest (most strokes visible on dark bg).
pub fn lookup(density: f32, dir: Direction) -> char {
    let table: &[(f32, char)] = match dir {
        Direction::Uniform => &UNIFORM,
        Direction::Horizontal => &HORIZONTAL,
        Direction::Vertical => &VERTICAL,
        Direction::DiagRight => &DIAG_RIGHT,
        Direction::DiagLeft => &DIAG_LEFT,
    };

    let mut best_ch = table[0].1;
    let mut best_dist = (density - table[0].0).abs();
    for &(d, ch) in &table[1..] {
        let dist = (density - d).abs();
        if dist < best_dist {
            best_dist = dist;
            best_ch = ch;
        }
    }
    best_ch
}

// ── Character tables ────────────────────────────────────────────────────
// (density, char) sorted by density.  Density = visual weight on screen.
// Characters are chosen so their stroke patterns suggest the given direction.

/// Symmetric / boxy characters for areas with no dominant edge.
const UNIFORM: [(f32, char); 10] = [
    (0.00, '\u{3000}'), // full-width space
    (0.10, '。'),
    (0.20, '口'),
    (0.30, '日'),
    (0.40, '目'),
    (0.50, '田'),
    (0.60, '面'),
    (0.72, '里'),
    (0.85, '圏'),
    (1.00, '鬱'),
];

/// Characters with dominant horizontal strokes.
const HORIZONTAL: [(f32, char); 10] = [
    (0.00, '\u{3000}'),
    (0.10, '一'),
    (0.20, '二'),
    (0.30, '三'),
    (0.40, '工'),
    (0.50, '王'),
    (0.60, '亜'),
    (0.72, '直'),
    (0.85, '書'),
    (1.00, '議'),
];

/// Characters with dominant vertical strokes.
const VERTICAL: [(f32, char); 10] = [
    (0.00, '\u{3000}'),
    (0.10, '丁'),
    (0.20, '川'),
    (0.30, '仙'),
    (0.40, '竹'),
    (0.50, '州'),
    (0.60, '冊'),
    (0.72, '制'),
    (0.85, '側'),
    (1.00, '欄'),
];

/// Characters with dominant / (right-upward) strokes.
const DIAG_RIGHT: [(f32, char); 10] = [
    (0.00, '\u{3000}'),
    (0.10, 'ノ'),
    (0.20, '八'),
    (0.30, '彡'),
    (0.40, '杉'),
    (0.50, '多'),
    (0.60, '移'),
    (0.72, '彩'),
    (0.85, '影'),
    (1.00, '繊'),
];

/// Characters with dominant \ (left-downward) strokes.
const DIAG_LEFT: [(f32, char); 10] = [
    (0.00, '\u{3000}'),
    (0.10, 'し'),
    (0.20, '入'),
    (0.30, '之'),
    (0.40, '久'),
    (0.50, '反'),
    (0.60, '及'),
    (0.72, '後'),
    (0.85, '投'),
    (1.00, '醸'),
];
