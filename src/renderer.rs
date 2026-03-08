use std::io::Write;

use image::RgbImage;

#[derive(Clone, Copy)]
pub enum Mode {
    Ascii,
    Color,
    Braille,
    Kanji,
}

pub struct Renderer {
    mode: Mode,
    chars: Vec<char>,
    buf: String,
    lum_buf: Vec<u8>,
}

impl Renderer {
    pub fn new(mode: Mode, chars: &str) -> Self {
        Self {
            mode,
            chars: chars.chars().collect(),
            buf: String::with_capacity(1 << 16),
            lum_buf: Vec::new(),
        }
    }

    /// Calculate pixel dimensions needed for the terminal size, preserving aspect ratio.
    /// `src_w`/`src_h` are source dimensions (0 means fill terminal).
    /// In Color mode, each cell shows 2 vertical pixels using half-block chars.
    /// In Ascii mode, each cell shows 1 pixel with ~2:1 aspect correction.
    pub fn target_size(&self, cols: u16, rows: u16) -> (u16, u16) {
        match self.mode {
            Mode::Color => (cols, rows * 2),
            Mode::Ascii => (cols, rows),
            Mode::Braille => (cols * 2, rows * 4),
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
            Mode::Color => rows as u32 * 2,
            Mode::Ascii => rows as u32,
            Mode::Braille => rows as u32 * 4,
            Mode::Kanji => rows as u32 * cell,
        };
        let pixel_cols = match self.mode {
            Mode::Braille => cols as u32 * 2,
            Mode::Kanji => (cols as u32 / 2) * cell,
            _ => cols as u32,
        };

        // Terminal chars are roughly 1:2 (width:height).
        // In color mode, half-blocks compensate vertically.
        // In ascii mode, we need aspect correction: each char covers ~2x height vs width.
        // In braille mode, 2x4 dots per cell → effective aspect is 1:1.
        let aspect_correction = match self.mode {
            Mode::Color | Mode::Braille | Mode::Kanji => 1.0_f64,
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

        match self.mode {
            Mode::Ascii => {
                self.render_ascii(tw, th, stride, rgb, cols);
            }
            Mode::Color => {
                self.render_color(tw, th, stride, rgb, cols);
            }
            Mode::Braille => {
                self.render_braille(tw, th, stride, rgb, cols);
            }
            Mode::Kanji => {
                self.render_kanji(tw, th, stride, rgb);
            }
        }

        w.write_all(self.buf.as_bytes())
    }

    fn render_ascii(
        &mut self,
        tw: usize,
        th: usize,
        stride: usize,
        rgb: &[u8],
        cols: usize,
    ) {
        let ramp_len = self.chars.len();

        for y in 0..th {
            let row_off = y * stride;
            let line_width = tw.min(cols);
            for x in 0..line_width {
                let off = row_off + x * 3;
                if off + 2 >= rgb.len() {
                    self.buf.push(' ');
                    continue;
                }
                let r = rgb[off] as u32;
                let g = rgb[off + 1] as u32;
                let b = rgb[off + 2] as u32;
                // Fast luminance approximation
                let lum = (r * 77 + g * 150 + b * 29) >> 8;
                let idx = (lum as usize * (ramp_len - 1)) / 255;
                self.buf.push(self.chars[idx]);
            }
            if y + 1 < th {
                self.buf.push_str("\x1b[0K\r\n");
            }
        }
        // Clear to end of screen
        self.buf.push_str("\x1b[0K\x1b[J");
    }

    fn render_color(
        &mut self,
        tw: usize,
        th: usize,
        stride: usize,
        rgb: &[u8],
        cols: usize,
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

                let fg = (tr, tg, tb);
                let bg_color = (br, bg, bb);

                // Only emit escape codes when color changes
                if first || fg != prev_fg || bg_color != prev_bg {
                    use std::fmt::Write;
                    let _ = write!(
                        self.buf,
                        "\x1b[38;2;{};{};{};48;2;{};{};{}m",
                        fg.0, fg.1, fg.2, bg_color.0, bg_color.1, bg_color.2
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
    ) {
        // Braille: each terminal cell = 2x4 pixel grid
        // Dot bit mapping: (dx, dy, bit)
        const DOTS: [(usize, usize, u8); 8] = [
            (0, 0, 1),   (0, 1, 2),   (0, 2, 4),   (0, 3, 64),
            (1, 0, 8),   (1, 1, 16),  (1, 2, 32),  (1, 3, 128),
        ];

        // 4x4 Bayer ordered dithering matrix (values 0..255)
        const BAYER4: [[u8; 4]; 4] = [
            [  0, 128,  32, 160],
            [192,  64, 224,  96],
            [ 48, 176,  16, 144],
            [240, 112, 208,  80],
        ];

        // Gamma LUT to brighten dark areas (gamma ≈ 0.6)
        // This compensates for braille's binary dots making dark areas too black.
        let gamma_lut: [u8; 256] = std::array::from_fn(|i| {
            ((i as f32 / 255.0).powf(0.6) * 255.0) as u8
        });

        let cell_cols = (tw / 2).min(cols);
        let cell_rows = th / 4;

        for cy in 0..cell_rows {
            let mut prev_fg = (0u8, 0u8, 0u8);
            let mut first = true;

            for cx in 0..cell_cols {
                let x0 = cx * 2;
                let y0 = cy * 4;
                let mut bits: u8 = 0;
                let mut sr: u32 = 0;
                let mut sg: u32 = 0;
                let mut sb: u32 = 0;
                let mut count: u32 = 0;

                for &(dx, dy, bit) in &DOTS {
                    let px = x0 + dx;
                    let py = y0 + dy;
                    let off = py * stride + px * 3;
                    let (r, g, b) = if off + 2 < rgb.len() {
                        (rgb[off] as u32, rgb[off + 1] as u32, rgb[off + 2] as u32)
                    } else {
                        (0, 0, 0)
                    };
                    let lum = (r * 77 + g * 150 + b * 29) >> 8;
                    let lum = gamma_lut[lum as usize] as u32;
                    let threshold = BAYER4[py % 4][px % 4] as u32;
                    if lum > threshold {
                        bits |= bit;
                        sr += r;
                        sg += g;
                        sb += b;
                        count += 1;
                    }
                }

                if count > 0 {
                    let fg = (
                        (sr / count) as u8,
                        (sg / count) as u8,
                        (sb / count) as u8,
                    );
                    if first || fg != prev_fg {
                        use std::fmt::Write;
                        let _ = write!(
                            self.buf,
                            "\x1b[38;2;{};{};{}m",
                            fg.0, fg.1, fg.2
                        );
                        prev_fg = fg;
                        first = false;
                    }
                } else if !first {
                    self.buf.push_str("\x1b[0m");
                    first = true;
                }

                let ch = char::from_u32(0x2800 + bits as u32).unwrap_or(' ');
                self.buf.push(ch);
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
    ) {
        use crate::kanji::{self, CELL};

        let kanji_cols = tw / CELL;
        let kanji_rows = th / CELL;
        let half = CELL / 2;
        let q_pixels = (half * half) as f32;

        // Build luminance buffer (reused across frames)
        self.lum_buf.resize(tw * th, 0);
        for y in 0..th {
            let row_off = y * stride;
            for x in 0..tw {
                let off = row_off + x * 3;
                if off + 2 < rgb.len() {
                    let r = rgb[off] as u32;
                    let g = rgb[off + 1] as u32;
                    let b = rgb[off + 2] as u32;
                    self.lum_buf[y * tw + x] = ((r * 77 + g * 150 + b * 29) >> 8) as u8;
                } else {
                    self.lum_buf[y * tw + x] = 0;
                }
            }
        }

        for cy in 0..kanji_rows {
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
                self.buf.push(ch);
            }
            if cy + 1 < kanji_rows {
                self.buf.push_str("\x1b[0K\r\n");
            }
        }
        self.buf.push_str("\x1b[0K\x1b[J");
    }
}
