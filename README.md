# aa-media

A terminal-based media viewer that renders images and videos as ASCII/Unicode art.

## Installation

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (edition 2024)
- `ffmpeg` and `ffprobe` on PATH (required for video playback)

### Build from source

```bash
git clone https://github.com/S-Nakamur-a/aa-media.git
cd aa-media
cargo install --path .
```

## Usage

```bash
aa-media <file-or-url> [OPTIONS]
```

### Input sources

- Local image/video files
- Image URLs
- YouTube URLs
- Web page URLs (rendered as screenshot)

### Options

| Option | Description | Default |
|--------|-------------|---------|
| `-m, --mode <MODE>` | Rendering mode: `ascii`, `tile`, `braille`, `kanji` | `braille` |
| `--color` | Enable color output | grayscale |
| `--chars <CHARS>` | Custom ASCII character ramp (darkest to brightest) | ` .:-=+*#%@` |

### Examples

```bash
# Display an image in braille mode
aa-media photo.jpg

# Display with color
aa-media photo.png --color

# Use ASCII mode with custom characters
aa-media photo.jpg -m ascii --chars " .:oO@"

# Play a video
aa-media video.mp4 --color

# Display from a URL
aa-media https://example.com/image.png

# Play a YouTube video
aa-media "https://www.youtube.com/watch?v=..."
```

### Keybindings

| Key | Action |
|-----|--------|
| `q` / `Esc` / `Ctrl+C` | Quit |
| `←` / `→` | Seek ±5 seconds (video) |
| `Shift+←` / `Shift+→` | Seek ±30 seconds (video) |

## License

MIT
