use std::io::Write;

use image::RgbImage;

/// 4x4 Bayer ordered dithering matrix (values 0..255)
const BAYER4: [[u8; 4]; 4] = [
    [  0, 128,  32, 160],
    [192,  64, 224,  96],
    [ 48, 176,  16, 144],
    [240, 112, 208,  80],
];

#[derive(Clone, Copy)]
pub enum Mode {
    Ascii,
    Tile,
    Braille,
    Sextant,
    Octant,
    Kanji,
}

pub struct Renderer {
    mode: Mode,
    color: bool,
    chars: Vec<char>,
    buf: String,
    lum_buf: Vec<u8>,
    char_lut: [u8; 256],
    color_lut: [u8; 256],
}

impl Renderer {
    pub fn new(mode: Mode, chars: &str, color: bool) -> Self {
        Self {
            mode,
            color,
            chars: chars.chars().collect(),
            buf: String::with_capacity(1 << 16),
            lum_buf: Vec::new(),
            char_lut: std::array::from_fn(|i| {
                ((i as f32 / 255.0).powf(0.7) * 255.0) as u8
            }),
            color_lut: std::array::from_fn(|i| {
                ((i as f32 / 255.0).powf(0.6) * 255.0) as u8
            }),
        }
    }

    /// Calculate pixel dimensions needed for the terminal size, preserving aspect ratio.
    /// `src_w`/`src_h` are source dimensions (0 means fill terminal).
    /// In Tile mode, each cell shows 2 vertical pixels using half-block chars.
    /// In Ascii mode, each cell shows 1 pixel with ~2:1 aspect correction.
    pub fn target_size(&self, cols: u16, rows: u16) -> (u16, u16) {
        match self.mode {
            Mode::Tile => (cols, rows * 2),
            Mode::Ascii => (cols, rows),
            Mode::Sextant => (cols * 2, rows * 3),
            Mode::Braille | Mode::Octant => (cols * 2, rows * 4),
            // Each kanji = 2 terminal columns; CELL×CELL pixels per kanji cell
            Mode::Kanji => {
                let cell = crate::kanji::CELL as u16;
                ((cols / 2) * cell, rows * cell)
            }
        }
    }

    /// Like target_size but fits source aspect ratio into the terminal.
    pub fn target_size_fit(&self, cols: u16, rows: u16, src_w: u32, src_h: u32) -> (u16, u16) {
        if src_w == 0 || src_h == 0 {
            return self.target_size(cols, rows);
        }

        // Pixel height available
        let cell = crate::kanji::CELL as u32;
        let pixel_rows = match self.mode {
            Mode::Tile => rows as u32 * 2,
            Mode::Ascii => rows as u32,
            Mode::Sextant => rows as u32 * 3,
            Mode::Braille | Mode::Octant => rows as u32 * 4,
            Mode::Kanji => rows as u32 * cell,
        };
        let pixel_cols = match self.mode {
            Mode::Sextant | Mode::Braille | Mode::Octant => cols as u32 * 2,
            Mode::Kanji => (cols as u32 / 2) * cell,
            _ => cols as u32,
        };

        // Terminal chars are roughly 1:2 (width:height).
        // Aspect correction = sub_pixel_h / (2 * sub_pixel_w) = ph / (2 * pw)
        // where pw×ph is the sub-pixel grid per cell.
        let aspect_correction = match self.mode {
            Mode::Tile | Mode::Braille | Mode::Octant | Mode::Kanji => 1.0_f64,
            Mode::Sextant => 0.75, // 2×3 grid: 3/(2*2) = 0.75
            Mode::Ascii => 0.5,
        };

        let src_aspect = src_w as f64 / (src_h as f64 * aspect_correction);
        let term_aspect = pixel_cols as f64 / pixel_rows as f64;

        let (tw, th) = if src_aspect > term_aspect {
            // Width-limited
            let tw = pixel_cols;
            let th = (pixel_cols as f64 / src_aspect) as u32;
            (tw, th)
        } else {
            // Height-limited
            let th = pixel_rows;
            let tw = (pixel_rows as f64 * src_aspect) as u32;
            (tw, th)
        };

        (tw.max(1).min(pixel_cols) as u16, th.max(1).min(pixel_rows) as u16)
    }

    /// Render an image::RgbImage frame (used for static images).
    pub fn render_frame<W: Write>(
        &mut self,
        w: &mut W,
        img: &RgbImage,
        _cols: u16,
    ) -> std::io::Result<()> {
        let width = img.width() as usize;
        let height = img.height() as usize;
        let pixels = img.as_raw();
        self.render_rgb_buffer(w, pixels, width as u16, height as u16, width as u16)
    }

    /// Render raw RGB buffer. This is the hot path for video.
    /// `tw` x `th` = pixel dimensions, `cols` = terminal columns for line padding.
    pub fn render_rgb_buffer<W: Write>(
        &mut self,
        w: &mut W,
        rgb: &[u8],
        tw: u16,
        th: u16,
        cols: u16,
    ) -> std::io::Result<()> {
        self.buf.clear();

        let tw = tw as usize;
        let th = th as usize;
        let cols = cols as usize;
        let stride = tw * 3;

        // Calculate horizontal padding for centering
        let rendered_cols = match self.mode {
            Mode::Ascii | Mode::Tile => tw.min(cols),
            Mode::Braille | Mode::Octant | Mode::Sextant => (tw / 2).min(cols),
            Mode::Kanji => {
                let cell = crate::kanji::CELL;
                (tw / cell) * 2 // each kanji = 2 terminal columns
            }
        };
        let pad = if cols > rendered_cols {
            (cols - rendered_cols) / 2
        } else {
            0
        };

        match self.mode {
            Mode::Ascii => self.render_ascii(tw, th, stride, rgb, cols, pad),
            Mode::Tile => self.render_tile(tw, th, stride, rgb, cols, pad),
            Mode::Braille => self.render_braille(tw, th, stride, rgb, cols, pad),
            Mode::Sextant => self.render_sextant(tw, th, stride, rgb, cols, pad),
            Mode::Octant => self.render_octant(tw, th, stride, rgb, cols, pad),
            Mode::Kanji => self.render_kanji(tw, th, stride, rgb, pad),
        }

        w.write_all(self.buf.as_bytes())
    }

    fn pad_line(&mut self, pad: usize) {
        self.buf.extend(std::iter::repeat_n(' ', pad));
    }

    fn render_ascii(
        &mut self,
        tw: usize,
        th: usize,
        stride: usize,
        rgb: &[u8],
        cols: usize,
        pad: usize,
    ) {
        let ramp_len = self.chars.len();

        for y in 0..th {
            let row_off = y * stride;
            let line_width = tw.min(cols);
            let mut prev_fg = (0u8, 0u8, 0u8);
            let mut first = true;
            self.pad_line(pad);
            for x in 0..line_width {
                let off = row_off + x * 3;
                if off + 2 >= rgb.len() {
                    self.buf.push(' ');
                    continue;
                }
                let r = rgb[off] as u32;
                let g = rgb[off + 1] as u32;
                let b = rgb[off + 2] as u32;
                // Character selection: mildly corrected luminance
                let lum = ((r * 77 + g * 150 + b * 29) >> 8) as usize;
                let corrected_lum = self.char_lut[lum] as usize;
                let idx = (corrected_lum * (ramp_len - 1)) / 255;
                if self.color {
                    // Color output: scale RGB by stronger gamma ratio
                    let fg = boost_color(r, g, b, lum, &self.color_lut);
                    if first || fg != prev_fg {
                        push_fg(&mut self.buf, fg.0, fg.1, fg.2);
                        prev_fg = fg;
                        first = false;
                    }
                }
                self.buf.push(self.chars[idx]);
            }
            if self.color {
                self.buf.push_str("\x1b[0m");
            }
            if y + 1 < th {
                self.buf.push_str("\x1b[0K\r\n");
            }
        }
        // Clear to end of screen
        self.buf.push_str("\x1b[0K\x1b[J");
    }

    fn render_tile(
        &mut self,
        tw: usize,
        th: usize,
        stride: usize,
        rgb: &[u8],
        cols: usize,
        pad: usize,
    ) {
        // Half-block rendering: each terminal row shows 2 pixel rows
        // Top pixel = foreground color, bottom pixel = background color, char = '▀'
        let rows = th / 2;

        for row in 0..rows {
            let top_y = row * 2;
            let bot_y = top_y + 1;
            let top_off = top_y * stride;
            let bot_off = bot_y * stride;
            let line_width = tw.min(cols);

            let mut prev_fg = (0u8, 0u8, 0u8);
            let mut prev_bg = (0u8, 0u8, 0u8);
            let mut first = true;

            self.pad_line(pad);
            for x in 0..line_width {
                let t = top_off + x * 3;
                let b = bot_off + x * 3;

                let (tr, tg, tb) = if t + 2 < rgb.len() {
                    (rgb[t], rgb[t + 1], rgb[t + 2])
                } else {
                    (0, 0, 0)
                };
                let (br, bg, bb) = if b + 2 < rgb.len() {
                    (rgb[b], rgb[b + 1], rgb[b + 2])
                } else {
                    (0, 0, 0)
                };

                let fg = if self.color {
                    (tr, tg, tb)
                } else {
                    let l = ((tr as u32 * 77 + tg as u32 * 150 + tb as u32 * 29) >> 8) as u8;
                    (l, l, l)
                };
                let bg_color = if self.color {
                    (br, bg, bb)
                } else {
                    let l = ((br as u32 * 77 + bg as u32 * 150 + bb as u32 * 29) >> 8) as u8;
                    (l, l, l)
                };

                // Only emit escape codes when color changes
                if first || fg != prev_fg || bg_color != prev_bg {
                    push_fg_bg(
                        &mut self.buf,
                        fg.0, fg.1, fg.2,
                        bg_color.0, bg_color.1, bg_color.2,
                    );
                    prev_fg = fg;
                    prev_bg = bg_color;
                    first = false;
                }
                self.buf.push('▀');
            }
            self.buf.push_str("\x1b[0m");
            if row + 1 < rows {
                self.buf.push_str("\x1b[0K\r\n");
            }
        }
        self.buf.push_str("\x1b[0K\x1b[J");
    }

    fn render_braille(
        &mut self,
        tw: usize,
        th: usize,
        stride: usize,
        rgb: &[u8],
        cols: usize,
        pad: usize,
    ) {
        // Braille: each terminal cell = 2x4 pixel grid
        // Dot bit mapping: (dx, dy, bit)
        const DOTS: [(usize, usize, u8); 8] = [
            (0, 0, 1),   (0, 1, 2),   (0, 2, 4),   (0, 3, 64),
            (1, 0, 8),   (1, 1, 16),  (1, 2, 32),  (1, 3, 128),
        ];

        let cell_cols = (tw / 2).min(cols);
        let cell_rows = th / 4;

        for cy in 0..cell_rows {
            let mut prev_fg = (0u8, 0u8, 0u8);
            let mut first = true;

            self.pad_line(pad);
            for cx in 0..cell_cols {
                let x0 = cx * 2;
                let y0 = cy * 4;
                let mut bits: u8 = 0;
                let mut sr: u32 = 0;
                let mut sg: u32 = 0;
                let mut sb: u32 = 0;
                let mut lum_min: u32 = 255;
                let mut lum_max: u32 = 0;

                // First pass: collect luminance range and color sums
                let mut dot_lums: [u32; 8] = [0; 8];
                for (i, &(dx, dy, _bit)) in DOTS.iter().enumerate() {
                    let px = x0 + dx;
                    let py = y0 + dy;
                    let off = py * stride + px * 3;
                    let (r, g, b) = if off + 2 < rgb.len() {
                        (rgb[off] as u32, rgb[off + 1] as u32, rgb[off + 2] as u32)
                    } else {
                        (0, 0, 0)
                    };
                    let lum = (r * 77 + g * 150 + b * 29) >> 8;
                    dot_lums[i] = lum;
                    lum_min = lum_min.min(lum);
                    lum_max = lum_max.max(lum);
                    sr += r;
                    sg += g;
                    sb += b;
                }

                // Smooth threshold reduction based on cell uniformity.
                // Low variance → reduce Bayer thresholds (dots turn ON easier).
                // High variance → normal dithering.
                let variance = lum_max - lum_min;
                // uniformity: 1.0 (perfectly uniform) → 0.0 (high variance)
                let uniformity = 1.0 - (variance as f32 / 128.0).min(1.0);
                // Scale down thresholds: uniform areas get thresholds * (1 - uniformity²)
                let threshold_scale = 1.0 - uniformity * uniformity;

                for (i, &(dx, dy, bit)) in DOTS.iter().enumerate() {
                    let py = y0 + dy;
                    let px = x0 + dx;
                    let corrected_lum = self.char_lut[dot_lums[i] as usize] as u32;
                    let base_threshold = BAYER4[py % 4][px % 4] as f32;
                    let threshold = (base_threshold * threshold_scale) as u32;
                    if corrected_lum > threshold {
                        bits |= bit;
                    }
                }

                if self.color {
                    // Color: boost by stronger gamma ratio (DOTS always has 8 elements)
                    let avg_r = sr / 8;
                    let avg_g = sg / 8;
                    let avg_b = sb / 8;
                    let avg_lum = ((avg_r * 77 + avg_g * 150 + avg_b * 29) >> 8) as usize;
                    let fg = boost_color(avg_r, avg_g, avg_b, avg_lum, &self.color_lut);
                    if first || fg != prev_fg {
                        push_fg(&mut self.buf, fg.0, fg.1, fg.2);
                        prev_fg = fg;
                        first = false;
                    }
                } else if !first {
                    self.buf.push_str("\x1b[0m");
                    first = true;
                }

                // SAFETY: 0x2800 + (0..=255) is always a valid Unicode scalar value
                let ch = unsafe { char::from_u32_unchecked(0x2800 + bits as u32) };
                self.buf.push(ch);
            }
            self.buf.push_str("\x1b[0m");
            if cy + 1 < cell_rows {
                self.buf.push_str("\x1b[0K\r\n");
            }
        }
        self.buf.push_str("\x1b[0K\x1b[J");
    }

    fn render_sextant(
        &mut self,
        tw: usize,
        th: usize,
        stride: usize,
        rgb: &[u8],
        cols: usize,
        pad: usize,
    ) {
        let cell_cols = (tw / 2).min(cols);
        let cell_rows = th / 3;

        for cy in 0..cell_rows {
            let mut prev_fg = (0u8, 0u8, 0u8);
            let mut prev_bg = (0u8, 0u8, 0u8);
            let mut first = true;

            self.pad_line(pad);
            for cx in 0..cell_cols {
                let x0 = cx * 2;
                let y0 = cy * 3;
                let mut bits: u8 = 0;
                let mut fg_r: u32 = 0; let mut fg_g: u32 = 0; let mut fg_b: u32 = 0; let mut fg_n: u32 = 0;
                let mut bg_r: u32 = 0; let mut bg_g: u32 = 0; let mut bg_b: u32 = 0; let mut bg_n: u32 = 0;

                // Row-major bit mapping: bit = dy * 2 + dx
                for dy in 0..3usize {
                    for dx in 0..2usize {
                        let px = x0 + dx;
                        let py = y0 + dy;
                        let off = py * stride + px * 3;
                        let (r, g, b) = if off + 2 < rgb.len() {
                            (rgb[off] as u32, rgb[off + 1] as u32, rgb[off + 2] as u32)
                        } else {
                            (0, 0, 0)
                        };
                        let lum = (r * 77 + g * 150 + b * 29) >> 8;
                        let threshold = BAYER4[py % 4][px % 4] as u32;
                        if lum > threshold {
                            bits |= 1u8 << (dy * 2 + dx);
                            fg_r += r; fg_g += g; fg_b += b; fg_n += 1;
                        } else {
                            bg_r += r; bg_g += g; bg_b += b; bg_n += 1;
                        }
                    }
                }

                if self.color {
                    let fg = if fg_n > 0 {
                        ((fg_r / fg_n) as u8, (fg_g / fg_n) as u8, (fg_b / fg_n) as u8)
                    } else { (0, 0, 0) };
                    let bg = if bg_n > 0 {
                        ((bg_r / bg_n) as u8, (bg_g / bg_n) as u8, (bg_b / bg_n) as u8)
                    } else { (0, 0, 0) };
                    if first || fg != prev_fg || bg != prev_bg {
                        push_fg_bg(
                            &mut self.buf,
                            fg.0, fg.1, fg.2,
                            bg.0, bg.1, bg.2,
                        );
                        prev_fg = fg;
                        prev_bg = bg;
                        first = false;
                    }
                }

                self.buf.push(sextant_char(bits));
            }
            self.buf.push_str("\x1b[0m");
            if cy + 1 < cell_rows {
                self.buf.push_str("\x1b[0K\r\n");
            }
        }
        self.buf.push_str("\x1b[0K\x1b[J");
    }

    fn render_octant(
        &mut self,
        tw: usize,
        th: usize,
        stride: usize,
        rgb: &[u8],
        cols: usize,
        pad: usize,
    ) {
        let cell_cols = (tw / 2).min(cols);
        let cell_rows = th / 4;

        for cy in 0..cell_rows {
            let mut prev_fg = (0u8, 0u8, 0u8);
            let mut prev_bg = (0u8, 0u8, 0u8);
            let mut first = true;

            self.pad_line(pad);
            for cx in 0..cell_cols {
                let x0 = cx * 2;
                let y0 = cy * 4;
                let mut bits: u8 = 0;
                let mut fg_r: u32 = 0; let mut fg_g: u32 = 0; let mut fg_b: u32 = 0; let mut fg_n: u32 = 0;
                let mut bg_r: u32 = 0; let mut bg_g: u32 = 0; let mut bg_b: u32 = 0; let mut bg_n: u32 = 0;

                // Row-major bit mapping: bit = dy * 2 + dx
                for dy in 0..4usize {
                    for dx in 0..2usize {
                        let px = x0 + dx;
                        let py = y0 + dy;
                        let off = py * stride + px * 3;
                        let (r, g, b) = if off + 2 < rgb.len() {
                            (rgb[off] as u32, rgb[off + 1] as u32, rgb[off + 2] as u32)
                        } else {
                            (0, 0, 0)
                        };
                        let lum = (r * 77 + g * 150 + b * 29) >> 8;
                        let threshold = BAYER4[py % 4][px % 4] as u32;
                        if lum > threshold {
                            bits |= 1u8 << (dy * 2 + dx);
                            fg_r += r; fg_g += g; fg_b += b; fg_n += 1;
                        } else {
                            bg_r += r; bg_g += g; bg_b += b; bg_n += 1;
                        }
                    }
                }

                if self.color {
                    let fg = if fg_n > 0 {
                        ((fg_r / fg_n) as u8, (fg_g / fg_n) as u8, (fg_b / fg_n) as u8)
                    } else { (0, 0, 0) };
                    let bg = if bg_n > 0 {
                        ((bg_r / bg_n) as u8, (bg_g / bg_n) as u8, (bg_b / bg_n) as u8)
                    } else { (0, 0, 0) };
                    if first || fg != prev_fg || bg != prev_bg {
                        push_fg_bg(
                            &mut self.buf,
                            fg.0, fg.1, fg.2,
                            bg.0, bg.1, bg.2,
                        );
                        prev_fg = fg;
                        prev_bg = bg;
                        first = false;
                    }
                }

                self.buf.push(octant_char(bits));
            }
            self.buf.push_str("\x1b[0m");
            if cy + 1 < cell_rows {
                self.buf.push_str("\x1b[0K\r\n");
            }
        }
        self.buf.push_str("\x1b[0K\x1b[J");
    }

    fn render_kanji(
        &mut self,
        tw: usize,
        th: usize,
        stride: usize,
        rgb: &[u8],
        pad: usize,
    ) {
        use crate::kanji::{self, CELL};

        let kanji_cols = tw / CELL;
        let kanji_rows = th / CELL;
        let half = CELL / 2;
        let q_pixels = (half * half) as f32;

        // Build luminance buffer with mild gamma correction
        self.lum_buf.resize(tw * th, 0);
        for y in 0..th {
            let row_off = y * stride;
            for x in 0..tw {
                let off = row_off + x * 3;
                if off + 2 < rgb.len() {
                    let r = rgb[off] as u32;
                    let g = rgb[off + 1] as u32;
                    let b = rgb[off + 2] as u32;
                    let lum = ((r * 77 + g * 150 + b * 29) >> 8) as u8;
                    self.lum_buf[y * tw + x] = self.char_lut[lum as usize];
                } else {
                    self.lum_buf[y * tw + x] = 0;
                }
            }
        }

        let cell_pixels = (CELL * CELL) as u32;

        for cy in 0..kanji_rows {
            let mut prev_fg = (0u8, 0u8, 0u8);
            let mut first = true;

            self.pad_line(pad);
            for cx in 0..kanji_cols {
                let x0 = cx * CELL;
                let y0 = cy * CELL;

                // Split cell into 4 quadrants, compute average luminance of each
                let mut q = [0u32; 4]; // TL, TR, BL, BR
                for dy in 0..half {
                    for dx in 0..half {
                        q[0] += self.lum_buf[(y0 + dy) * tw + (x0 + dx)] as u32;
                        q[1] += self.lum_buf[(y0 + dy) * tw + (x0 + half + dx)] as u32;
                        q[2] += self.lum_buf[(y0 + half + dy) * tw + (x0 + dx)] as u32;
                        q[3] += self.lum_buf[(y0 + half + dy) * tw + (x0 + half + dx)] as u32;
                    }
                }

                let tl = q[0] as f32 / q_pixels;
                let tr = q[1] as f32 / q_pixels;
                let bl = q[2] as f32 / q_pixels;
                let br = q[3] as f32 / q_pixels;

                // Overall density (average luminance mapped to 0.0–1.0)
                let avg_lum = (tl + tr + bl + br) / 4.0;
                let density = avg_lum / 255.0;

                // Gradient from quadrant differences
                let gx = (tr + br) - (tl + bl); // horizontal change
                let gy = (bl + br) - (tl + tr); // vertical change

                let dir = kanji::classify(gx, gy);
                let ch = kanji::lookup(density, dir);

                if self.color {
                    // Average color over the cell with gamma correction
                    let mut sr: u32 = 0;
                    let mut sg: u32 = 0;
                    let mut sb: u32 = 0;
                    for dy in 0..CELL {
                        let row_off = (y0 + dy) * stride;
                        for dx in 0..CELL {
                            let off = row_off + (x0 + dx) * 3;
                            if off + 2 < rgb.len() {
                                sr += rgb[off] as u32;
                                sg += rgb[off + 1] as u32;
                                sb += rgb[off + 2] as u32;
                            }
                        }
                    }
                    let avg_r = sr / cell_pixels;
                    let avg_g = sg / cell_pixels;
                    let avg_b = sb / cell_pixels;
                    let avg_lum = ((avg_r * 77 + avg_g * 150 + avg_b * 29) >> 8) as usize;
                    let fg = boost_color(avg_r, avg_g, avg_b, avg_lum, &self.color_lut);
                    if first || fg != prev_fg {
                        push_fg(&mut self.buf, fg.0, fg.1, fg.2);
                        prev_fg = fg;
                        first = false;
                    }
                }

                self.buf.push(ch);
            }
            if self.color {
                self.buf.push_str("\x1b[0m");
            }
            if cy + 1 < kanji_rows {
                self.buf.push_str("\x1b[0K\r\n");
            }
        }
        self.buf.push_str("\x1b[0K\x1b[J");
    }
}

// ── Fast ANSI escape code writing ──────────────────────────────────────
// Bypasses std::fmt machinery for the hot path.

/// Push a u8 as decimal ASCII digits directly into the buffer.
#[inline(always)]
fn push_u8(buf: &mut String, n: u8) {
    if n >= 100 {
        buf.push((b'0' + n / 100) as char);
        buf.push((b'0' + (n / 10) % 10) as char);
        buf.push((b'0' + n % 10) as char);
    } else if n >= 10 {
        buf.push((b'0' + n / 10) as char);
        buf.push((b'0' + n % 10) as char);
    } else {
        buf.push((b'0' + n) as char);
    }
}

/// Write "\x1b[38;2;R;G;Bm" (foreground color) without fmt machinery.
#[inline(always)]
fn push_fg(buf: &mut String, r: u8, g: u8, b: u8) {
    buf.push_str("\x1b[38;2;");
    push_u8(buf, r);
    buf.push(';');
    push_u8(buf, g);
    buf.push(';');
    push_u8(buf, b);
    buf.push('m');
}

/// Write "\x1b[38;2;R;G;B;48;2;R;G;Bm" (fg + bg color) without fmt machinery.
#[inline(always)]
fn push_fg_bg(buf: &mut String, fr: u8, fg: u8, fb: u8, br: u8, bg: u8, bb: u8) {
    buf.push_str("\x1b[38;2;");
    push_u8(buf, fr);
    buf.push(';');
    push_u8(buf, fg);
    buf.push(';');
    push_u8(buf, fb);
    buf.push_str(";48;2;");
    push_u8(buf, br);
    buf.push(';');
    push_u8(buf, bg);
    buf.push(';');
    push_u8(buf, bb);
    buf.push('m');
}

// ── Character mapping functions ────────────────────────────────────────

/// Map a 6-bit sextant pattern (row-major 2×3 grid) to the corresponding Unicode character.
/// Grid bit layout:
///   bit0 | bit1
///   bit2 | bit3
///   bit4 | bit5
fn sextant_char(pattern: u8) -> char {
    match pattern & 0x3F {
        0 => ' ',
        21 => '\u{258C}', // ▌ LEFT HALF BLOCK
        42 => '\u{2590}', // ▐ RIGHT HALF BLOCK
        63 => '\u{2588}', // █ FULL BLOCK
        p => {
            // U+1FB00..U+1FB3B has 60 sextant chars for patterns 1..62 excluding 21 and 42
            let skip = if p > 42 { 2 } else if p > 21 { 1 } else { 0 };
            char::from_u32(0x1FB00 + (p as u32 - 1) - skip).unwrap()
        }
    }
}

/// Map an 8-bit octant pattern (row-major 2×4 grid) to the corresponding Unicode character.
/// Grid bit layout:
///   bit0 | bit1
///   bit2 | bit3
///   bit4 | bit5
///   bit6 | bit7
fn octant_char(pattern: u8) -> char {
    // 20 patterns already in Block Elements / Legacy Computing
    // 6 patterns in other SLC positions
    // 230 patterns sequentially at U+1CD00..U+1CDE5
    match pattern {
        0x00 => ' ',
        0x03 => '\u{1FB82}', // UPPER ONE QUARTER BLOCK
        0x05 => '\u{2598}',  // QUADRANT UPPER LEFT
        0x0A => '\u{259D}',  // QUADRANT UPPER RIGHT
        0x0F => '\u{2580}',  // UPPER HALF BLOCK
        0x3F => '\u{1FB85}', // UPPER THREE QUARTERS BLOCK
        0x50 => '\u{2596}',  // QUADRANT LOWER LEFT
        0x55 => '\u{258C}',  // LEFT HALF BLOCK
        0x5A => '\u{259E}',  // QUADRANT UPPER RIGHT AND LOWER LEFT
        0x5F => '\u{259B}',  // QUADRANT UPPER LEFT AND UPPER RIGHT AND LOWER LEFT
        0xA0 => '\u{2597}',  // QUADRANT LOWER RIGHT
        0xA5 => '\u{259A}',  // QUADRANT UPPER LEFT AND LOWER RIGHT
        0xAA => '\u{2590}',  // RIGHT HALF BLOCK
        0xAF => '\u{259C}',  // QUADRANT UPPER LEFT AND UPPER RIGHT AND LOWER RIGHT
        0xC0 => '\u{2582}',  // LOWER ONE QUARTER BLOCK
        0xF0 => '\u{2584}',  // LOWER HALF BLOCK
        0xF5 => '\u{2599}',  // QUADRANT UPPER LEFT AND LOWER LEFT AND LOWER RIGHT
        0xFA => '\u{259F}',  // QUADRANT UPPER RIGHT AND LOWER LEFT AND LOWER RIGHT
        0xFC => '\u{2586}',  // LOWER THREE QUARTERS BLOCK
        0xFF => '\u{2588}',  // FULL BLOCK
        // 6 patterns in other Legacy Computing positions
        0x01 => '\u{1CEA8}',
        0x02 => '\u{1CEAB}',
        0x14 => '\u{1FBE6}',
        0x28 => '\u{1FBE7}',
        0x40 => '\u{1CEA3}',
        0x80 => '\u{1CEA0}',
        // Remaining 230 patterns: U+1CD00 + sequential index
        p => {
            const SKIP: [u8; 26] = [
                0x00, 0x01, 0x02, 0x03, 0x05, 0x0A, 0x0F, 0x14, 0x28, 0x3F,
                0x40, 0x50, 0x55, 0x5A, 0x5F, 0x80, 0xA0, 0xA5, 0xAA, 0xAF,
                0xC0, 0xF0, 0xF5, 0xFA, 0xFC, 0xFF,
            ];
            // SKIP is sorted, so use binary search instead of linear filter
            let below = SKIP.partition_point(|&s| s < p) as u32;
            char::from_u32(0x1CD00 + p as u32 - below).unwrap()
        }
    }
}

/// Boost RGB color brightness using gamma-corrected luminance ratio.
/// Preserves color hue/saturation while increasing perceived brightness.
fn boost_color(r: u32, g: u32, b: u32, lum: usize, gamma_lut: &[u8; 256]) -> (u8, u8, u8) {
    let max_ch = r.max(g).max(b);
    if max_ch == 0 {
        return (0, 0, 0);
    }
    let boosted = gamma_lut[lum] as f32;
    let scale = (boosted / lum.max(1) as f32).min(255.0 / max_ch as f32);
    (
        (r as f32 * scale) as u8,
        (g as f32 * scale) as u8,
        (b as f32 * scale) as u8,
    )
}
