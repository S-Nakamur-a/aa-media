mod kanji;
mod renderer;
mod video;

use std::io::{self, Write};
use std::path::PathBuf;

use clap::Parser;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyModifiers},
    terminal,
};

use renderer::{Mode, Renderer};

#[derive(Parser)]
#[command(name = "aa-media", about = "Display images/videos as ASCII art in the terminal")]
struct Cli {
    /// Path to image or video file
    file: PathBuf,

    /// Rendering mode: ascii, color, braille, kanji
    #[arg(short, long, default_value = "braille")]
    mode: String,

    /// Custom ASCII character ramp (darkest to brightest)
    #[arg(long, default_value = " .:-=+*#%@")]
    chars: String,
}

fn main() {
    let cli = Cli::parse();

    let mode = match cli.mode.as_str() {
        "ascii" => Mode::Ascii,
        "braille" => Mode::Braille,
        "kanji" => Mode::Kanji,
        _ => Mode::Color,
    };

    let ext = cli
        .file
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let is_video = matches!(
        ext.as_str(),
        "mp4" | "mkv" | "avi" | "mov" | "webm" | "flv" | "wmv" | "m4v" | "ts" | "gif"
    );

    let result = if is_video {
        run_video(&cli.file, mode, &cli.chars)
    } else {
        run_image(&cli.file, mode, &cli.chars)
    };

    if let Err(e) = result {
        // Ensure terminal is restored on error
        let _ = crossterm::execute!(io::stdout(), terminal::LeaveAlternateScreen, cursor::Show);
        let _ = terminal::disable_raw_mode();
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

fn run_image(path: &PathBuf, mode: Mode, chars: &str) -> Result<(), Box<dyn std::error::Error>> {
    let img = image::open(path)?.into_rgb8();
    let src_w = img.width();
    let src_h = img.height();
    let mut stdout = io::BufWriter::with_capacity(1 << 16, io::stdout().lock());

    terminal::enable_raw_mode()?;
    crossterm::execute!(stdout, terminal::EnterAlternateScreen, cursor::Hide)?;

    let mut renderer = Renderer::new(mode, chars);

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

fn run_video(path: &PathBuf, mode: Mode, chars: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut stdout = io::BufWriter::with_capacity(1 << 17, io::stdout().lock());

    terminal::enable_raw_mode()?;
    crossterm::execute!(stdout, terminal::EnterAlternateScreen, cursor::Hide)?;

    let mut renderer = Renderer::new(mode, chars);
    let (src_w, src_h) = video::Player::probe_dimensions(path)?;
    let (cols, rows) = terminal::size()?;
    let (tw, th) = renderer.target_size_fit(cols, rows, src_w, src_h);

    let mut player = video::Player::new(path, tw, th)?;
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
                    let bar_width = cols_now as usize - 14; // space for time display
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
            // Sleep most of the time to save CPU, spin the last 2ms for precision
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
