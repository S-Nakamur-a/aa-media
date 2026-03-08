use std::io::Write;

use image::RgbImage;

#[derive(Clone, Copy)]
pub enum Mode {
    Ascii,
    Color,
}

pub struct Renderer {
    mode: Mode,
    chars: Vec<char>,
    buf: String,
}

impl Renderer {
    pub fn new(mode: Mode, chars: &str) -> Self {
        Self {
            mode,
            chars: chars.chars().collect(),
            buf: String::with_capacity(1 << 16),
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
        }
    }

    /// Like target_size but fits source aspect ratio into the terminal.
    pub fn target_size_fit(&self, cols: u16, rows: u16, src_w: u32, src_h: u32) -> (u16, u16) {
        if src_w == 0 || src_h == 0 {
            return self.target_size(cols, rows);
        }

        // Pixel height available
        let pixel_rows = match self.mode {
            Mode::Color => rows as u32 * 2,
            Mode::Ascii => rows as u32,
        };
        let pixel_cols = cols as u32;

        // Terminal chars are roughly 1:2 (width:height).
        // In color mode, half-blocks compensate vertically.
        // In ascii mode, we need aspect correction: each char covers ~2x height vs width.
        let aspect_correction = match self.mode {
            Mode::Color => 1.0_f64,
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
}
