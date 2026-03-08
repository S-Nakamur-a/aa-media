pub mod kanji;
pub mod renderer;
pub mod video;

use std::io::{self, Write};
use std::path::Path;

use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyModifiers},
    terminal,
};

pub use renderer::{Mode, Renderer};

/// Display an image in the terminal with interactive resize/quit support.
///
/// Loads the image from `path`, renders it using the specified `mode`,
/// and enters an event loop that handles terminal resize and quit (q/Esc/Ctrl-C).
pub fn run_image(path: &str, mode: Mode, chars: &str, color: bool) -> Result<(), Box<dyn std::error::Error>> {
    let img = image::open(Path::new(path))?.into_rgb8();
    let src_w = img.width();
    let src_h = img.height();
    let mut stdout = io::BufWriter::with_capacity(1 << 16, io::stdout().lock());

    terminal::enable_raw_mode()?;
    crossterm::execute!(stdout, terminal::EnterAlternateScreen, cursor::Hide)?;

    let mut renderer = Renderer::new(mode, chars, color);

    let render_image =
        |stdout: &mut io::BufWriter<io::StdoutLock>, renderer: &mut Renderer| -> Result<(), Box<dyn std::error::Error>> {
            let (cols, rows) = terminal::size()?;
            let (tw, th) = renderer.target_size_fit(cols, rows, src_w, src_h);
            let resized = image::imageops::resize(
                &img,
                tw as u32,
                th as u32,
                image::imageops::FilterType::Triangle,
            );
            write!(stdout, "\x1b[H\x1b[2J")?;
            renderer.render_frame(stdout, &resized, cols)?;
            stdout.flush()?;
            Ok(())
        };

    render_image(&mut stdout, &mut renderer)?;

    loop {
        if event::poll(std::time::Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) => {
                    if key.code == KeyCode::Char('q')
                        || key.code == KeyCode::Esc
                        || (key.code == KeyCode::Char('c')
                            && key.modifiers.contains(KeyModifiers::CONTROL))
                    {
                        break;
                    }
                }
                Event::Resize(_, _) => {
                    render_image(&mut stdout, &mut renderer)?;
                }
                _ => {}
            }
        }
    }

    crossterm::execute!(stdout, terminal::LeaveAlternateScreen, cursor::Show)?;
    terminal::disable_raw_mode()?;
    Ok(())
}

/// Play a video in the terminal with interactive seek/resize/quit support.
///
/// Streams frames from `source` via ffmpeg, renders using the specified `mode`,
/// and enters an event loop that handles seek (Left/Right), resize, and quit.
pub fn run_video(source: &str, mode: Mode, chars: &str, color: bool) -> Result<(), Box<dyn std::error::Error>> {
    let mut stdout = io::BufWriter::with_capacity(1 << 17, io::stdout().lock());

    terminal::enable_raw_mode()?;
    crossterm::execute!(stdout, terminal::EnterAlternateScreen, cursor::Hide)?;

    let mut renderer = Renderer::new(mode, chars, color);
    let (src_w, src_h) = video::Player::probe_dimensions(source)?;
    let (cols, rows) = terminal::size()?;
    let (tw, th) = renderer.target_size_fit(cols, rows, src_w, src_h);

    let mut player = video::Player::new(source, tw, th)?;
    let mut current_tw = tw;
    let mut current_th = th;
    let mut current_cols = cols;

    let frame_duration = player.frame_duration();

    loop {
        let frame_start = std::time::Instant::now();

        // Handle events (non-blocking)
        while event::poll(std::time::Duration::ZERO)? {
            match event::read()? {
                Event::Key(key) => {
                    if key.code == KeyCode::Char('q')
                        || key.code == KeyCode::Esc
                        || (key.code == KeyCode::Char('c')
                            && key.modifiers.contains(KeyModifiers::CONTROL))
                    {
                        crossterm::execute!(
                            stdout,
                            terminal::LeaveAlternateScreen,
                            cursor::Show
                        )?;
                        terminal::disable_raw_mode()?;
                        return Ok(());
                    }
                    let seek_amount = if key.modifiers.contains(KeyModifiers::SHIFT) {
                        30.0
                    } else {
                        5.0
                    };
                    match key.code {
                        KeyCode::Right => {
                            player.seek_relative(seek_amount)?;
                            write!(stdout, "\x1b[2J")?;
                        }
                        KeyCode::Left => {
                            player.seek_relative(-seek_amount)?;
                            write!(stdout, "\x1b[2J")?;
                        }
                        _ => {}
                    }
                }
                Event::Resize(c, r) => {
                    let (nw, nh) = renderer.target_size_fit(c, r, src_w, src_h);
                    if nw != current_tw || nh != current_th {
                        current_tw = nw;
                        current_th = nh;
                        current_cols = c;
                        player.resize(nw, nh)?;
                        write!(stdout, "\x1b[2J")?;
                    }
                }
                _ => {}
            }
        }

        // Read and render frame
        match player.next_frame()? {
            Some(frame) => {
                write!(stdout, "\x1b[H")?;
                renderer.render_rgb_buffer(&mut stdout, frame, current_tw, current_th, current_cols)?;

                // Draw progress bar on last row
                let (cols_now, rows_now) = terminal::size()?;
                let pos = player.position();
                let dur = player.duration();
                if dur > 0.0 {
                    let bar_width = (cols_now as usize).saturating_sub(14);
                    let filled = ((pos / dur) * bar_width as f64) as usize;
                    let pos_min = pos as u32 / 60;
                    let pos_sec = pos as u32 % 60;
                    let dur_min = dur as u32 / 60;
                    let dur_sec = dur as u32 % 60;
                    write!(
                        stdout,
                        "\x1b[{};1H\x1b[0m {pos_min:02}:{pos_sec:02} \x1b[7m{}\x1b[0m{} {dur_min:02}:{dur_sec:02}",
                        rows_now,
                        " ".repeat(filled.min(bar_width)),
                        " ".repeat(bar_width.saturating_sub(filled)),
                    )?;
                }

                stdout.flush()?;
            }
            None => break,
        }

        // Hybrid frame timing: sleep for bulk, spin for precision
        let elapsed = frame_start.elapsed();
        if elapsed < frame_duration {
            let remaining = frame_duration - elapsed;
            if remaining > std::time::Duration::from_millis(2) {
                std::thread::sleep(remaining - std::time::Duration::from_millis(2));
            }
            let target = frame_start + frame_duration;
            while std::time::Instant::now() < target {
                std::hint::spin_loop();
            }
        }
    }

    crossterm::execute!(stdout, terminal::LeaveAlternateScreen, cursor::Show)?;
    terminal::disable_raw_mode()?;
    Ok(())
}
