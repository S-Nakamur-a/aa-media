use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

/// Resolved input: either a local path or a resolved URL with optional temp storage.
pub struct ResolvedInput {
    /// Path or URL string to pass to image::open / ffmpeg.
    pub source: String,
    /// Whether this is a video source.
    pub is_video: bool,
    /// Keeps temp directory alive while we use the file inside it.
    _temp_dir: Option<TempDir>,
}

pub fn resolve(input: &str) -> Result<ResolvedInput, Box<dyn std::error::Error>> {
    if !input.starts_with("http://") && !input.starts_with("https://") {
        // Local file
        let ext = Path::new(input)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        let is_video = matches!(
            ext.as_str(),
            "mp4" | "mkv" | "avi" | "mov" | "webm" | "flv" | "wmv" | "m4v" | "ts" | "gif"
        );
        return Ok(ResolvedInput {
            source: input.to_string(),
            is_video,
            _temp_dir: None,
        });
    }

    // YouTube URL → get direct stream URL via yt-dlp
    if is_youtube_url(input) {
        eprintln!("Fetching video stream URL via yt-dlp...");
        let stream_url = yt_dlp_stream_url(input)?;
        return Ok(ResolvedInput {
            source: stream_url,
            is_video: true,
            _temp_dir: None,
        });
    }

    // Image URL → download to temp file
    if is_image_url(input) {
        eprintln!("Downloading image...");
        let tmp = TempDir::new()?;
        let dest = tmp.path().join("image");
        download(input, &dest)?;
        return Ok(ResolvedInput {
            source: dest.to_string_lossy().into_owned(),
            is_video: false,
            _temp_dir: Some(tmp),
        });
    }

    // Other URL → screenshot via headless browser
    eprintln!("Taking screenshot of webpage...");
    let tmp = TempDir::new()?;
    let dest = tmp.path().join("screenshot.png");
    screenshot(input, &dest)?;
    Ok(ResolvedInput {
        source: dest.to_string_lossy().into_owned(),
        is_video: false,
        _temp_dir: Some(tmp),
    })
}

fn is_youtube_url(url: &str) -> bool {
    // Strip scheme, then check host
    let host = url
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.");
    host.starts_with("youtube.com/")
        || host.starts_with("youtu.be/")
        || host.starts_with("m.youtube.com/")
}

fn is_image_url(url: &str) -> bool {
    // Check the URL path (before query string) for image extensions
    let path = url.split('?').next().unwrap_or(url);
    let path = path.split('#').next().unwrap_or(path);
    let lower = path.to_lowercase();
    lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".png")
        || lower.ends_with(".webp")
        || lower.ends_with(".bmp")
        || lower.ends_with(".gif")
}

fn yt_dlp_stream_url(url: &str) -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new("yt-dlp")
        .args([
            "--get-url",
            "-f",
            "best[height<=720]/best",
            "--no-playlist",
            url,
        ])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("yt-dlp failed: {stderr}").into());
    }

    let stream = String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .unwrap_or("")
        .to_string();

    if stream.is_empty() {
        return Err("yt-dlp returned no URL".into());
    }

    Ok(stream)
}

fn download(url: &str, dest: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let status = Command::new("curl")
        .args(["-fsSL", "-o"])
        .arg(dest)
        .arg(url)
        .status()?;

    if !status.success() {
        return Err(format!("curl failed to download {url}").into());
    }
    Ok(())
}

fn screenshot(url: &str, dest: &Path) -> Result<(), Box<dyn std::error::Error>> {
    // Try chromium-based browsers, then firefox
    let browsers = [
        ("chromium", chromium_args(url, dest)),
        ("chromium-browser", chromium_args(url, dest)),
        ("google-chrome", chromium_args(url, dest)),
        ("google-chrome-stable", chromium_args(url, dest)),
    ];

    for (bin, args) in &browsers {
        if let Ok(status) = Command::new(bin).args(args).status() {
            if status.success() && dest.exists() {
                return Ok(());
            }
        }
    }

    Err("No supported browser found for screenshots. Install chromium or google-chrome.".into())
}

fn chromium_args<'a>(url: &'a str, dest: &'a Path) -> Vec<String> {
    vec![
        "--headless=new".to_string(),
        format!("--screenshot={}", dest.display()),
        "--window-size=1280,720".to_string(),
        "--disable-gpu".to_string(),
        "--no-sandbox".to_string(),
        "--hide-scrollbars".to_string(),
        "--".to_string(),
        url.to_string(),
    ]
}
