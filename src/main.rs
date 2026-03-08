mod url;

use std::io;

use clap::Parser;
use crossterm::{cursor, terminal};

use aa_media::Mode;

#[derive(Parser)]
#[command(name = "aa-media", about = "Display images/videos as ASCII art in the terminal")]
struct Cli {
    /// Path or URL to image/video (supports local files, image URLs, YouTube URLs, and web pages)
    file: String,

    /// Rendering mode: ascii, tile, braille, sextant, octant, kanji
    #[arg(short, long, default_value = "braille")]
    mode: String,

    /// Disable color output (use grayscale)
    #[arg(long)]
    grayscale: bool,

    /// Custom ASCII character ramp (darkest to brightest)
    #[arg(long, default_value = " .:-=+*#%@")]
    chars: String,
}

fn main() {
    let cli = Cli::parse();

    let mode = match cli.mode.as_str() {
        "ascii" => Mode::Ascii,
        "braille" => Mode::Braille,
        "sextant" => Mode::Sextant,
        "octant" => Mode::Octant,
        "kanji" => Mode::Kanji,
        _ => Mode::Tile,
    };
    let color = !cli.grayscale;

    let resolved = match url::resolve(&cli.file) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error resolving input: {e}");
            std::process::exit(1);
        }
    };

    let result = if resolved.is_video {
        aa_media::run_video(&resolved.source, mode, &cli.chars, color)
    } else {
        aa_media::run_image(&resolved.source, mode, &cli.chars, color)
    };

    if let Err(e) = result {
        let _ = crossterm::execute!(io::stdout(), terminal::LeaveAlternateScreen, cursor::Show);
        let _ = terminal::disable_raw_mode();
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
