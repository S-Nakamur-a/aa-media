# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Run

```bash
cargo build                          # debug build
cargo build --release                # optimized release build
cargo run -- <file> [--mode <mode>]  # run directly
```

No tests or linter are configured. Uses Rust edition 2024.

## What This Is

A terminal-based media viewer that renders images and videos as ASCII/Unicode art. Supports four rendering modes (`--mode`):

- **ascii** — luminance-mapped characters from a configurable ramp (`--chars`)
- **color** — true-color half-block (`▀`) rendering, 2 vertical pixels per cell
- **braille** — Unicode braille patterns (2×4 dots per cell) with Bayer dithering and color
- **kanji** — maps pixel blocks to kanji characters by stroke density and edge direction

Video playback shells out to `ffmpeg`/`ffprobe` for decoding (required on PATH).

## Architecture

- **`main.rs`** — CLI parsing (clap), event loop for images (resize/quit) and videos (seek/resize/frame timing). Two entry paths: `run_image` (loads with `image` crate) and `run_video` (streams frames from ffmpeg).
- **`renderer.rs`** — `Renderer` struct with the core rendering pipeline. Converts RGB pixel buffers to terminal escape sequences. `render_rgb_buffer()` is the hot path for video. Handles aspect-ratio fitting via `target_size_fit()`.
- **`video.rs`** — `Player` struct wrapping an ffmpeg child process. Reads raw RGB24 frames from ffmpeg's stdout pipe. Supports seek (kills and respawns ffmpeg) and resize.
- **`kanji.rs`** — Kanji mode lookup tables. Maps (density, edge direction) pairs to visually appropriate kanji. `classify()` determines edge direction from gradient; `lookup()` finds nearest-density character.

## Key Design Details

- Terminal uses crossterm's alternate screen and raw mode; cleanup happens on quit and on error.
- Video frame timing uses hybrid sleep+spin-loop for precision (sleep bulk, spin last 2ms).
- Renderer reuses internal `String` buffer (`self.buf`) across frames to avoid allocation.
- Color escape codes are deduplicated (only emitted when color changes from previous pixel).
