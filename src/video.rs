use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

pub struct Player {
    child: Child,
    frame_buf: Vec<u8>,
    frame_size: usize, // tw * th * 3
    tw: u16,
    th: u16,
    fps: f64,
    source: String,
    position: f64,   // current playback position in seconds
    duration: f64,    // total duration in seconds
}

impl Player {
    pub fn new(source: &str, tw: u16, th: u16) -> Result<Self, Box<dyn std::error::Error>> {
        let fps = Self::probe_fps(source)?;
        let duration = Self::probe_duration(source)?;
        let frame_size = tw as usize * th as usize * 3;
        let child = Self::spawn_ffmpeg(source, tw, th, fps, 0.0)?;

        Ok(Self {
            child,
            frame_buf: vec![0u8; frame_size],
            frame_size,
            tw,
            th,
            fps,
            source: source.to_string(),
            position: 0.0,
            duration,
        })
    }

    pub fn frame_duration(&self) -> Duration {
        Duration::from_secs_f64(1.0 / self.fps)
    }

    pub fn duration(&self) -> f64 {
        self.duration
    }

    pub fn position(&self) -> f64 {
        self.position
    }

    /// Seek to a specific position in seconds.
    pub fn seek(&mut self, position: f64) -> Result<(), Box<dyn std::error::Error>> {
        let pos = position.max(0.0).min(self.duration);
        let _ = self.child.kill();
        let _ = self.child.wait();

        self.position = pos;
        self.child = Self::spawn_ffmpeg(&self.source, self.tw, self.th, self.fps, pos)?;
        Ok(())
    }

    /// Seek relative to current position.
    pub fn seek_relative(&mut self, delta: f64) -> Result<(), Box<dyn std::error::Error>> {
        self.seek(self.position + delta)
    }

    /// Restart ffmpeg with new dimensions on resize.
    pub fn resize(&mut self, tw: u16, th: u16) -> Result<(), Box<dyn std::error::Error>> {
        let _ = self.child.kill();
        let _ = self.child.wait();

        self.tw = tw;
        self.th = th;
        self.frame_size = tw as usize * th as usize * 3;
        self.frame_buf.resize(self.frame_size, 0);

        self.child = Self::spawn_ffmpeg(&self.source, tw, th, self.fps, self.position)?;
        Ok(())
    }

    /// Read the next frame as raw RGB bytes.
    pub fn next_frame(&mut self) -> Result<Option<&[u8]>, Box<dyn std::error::Error>> {
        let stdout = self
            .child
            .stdout
            .as_mut()
            .ok_or("ffmpeg stdout not available")?;

        // Read exactly one frame
        let mut total = 0;
        while total < self.frame_size {
            match stdout.read(&mut self.frame_buf[total..self.frame_size]) {
                Ok(0) => return Ok(None), // EOF
                Ok(n) => total += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e.into()),
            }
        }

        self.position += 1.0 / self.fps;

        Ok(Some(&self.frame_buf))
    }

    /// Probe video source dimensions (width, height).
    pub fn probe_dimensions(source: &str) -> Result<(u32, u32), Box<dyn std::error::Error>> {
        let output = Command::new("ffprobe")
            .args([
                "-v", "quiet",
                "-select_streams", "v:0",
                "-show_entries", "stream=width,height",
                "-of", "csv=p=0",
            ])
            .arg(source)
            .output()?;

        let s = String::from_utf8_lossy(&output.stdout);
        let s = s.trim();
        if let Some((w, h)) = s.split_once(',') {
            Ok((w.parse().unwrap_or(640), h.parse().unwrap_or(480)))
        } else {
            Ok((640, 480))
        }
    }

    fn probe_duration(source: &str) -> Result<f64, Box<dyn std::error::Error>> {
        let output = Command::new("ffprobe")
            .args([
                "-v", "quiet",
                "-show_entries", "format=duration",
                "-of", "csv=p=0",
            ])
            .arg(source)
            .output()?;

        let s = String::from_utf8_lossy(&output.stdout);
        Ok(s.trim().parse().unwrap_or(0.0))
    }

    fn probe_fps(source: &str) -> Result<f64, Box<dyn std::error::Error>> {
        let output = Command::new("ffprobe")
            .args([
                "-v", "quiet",
                "-select_streams", "v:0",
                "-show_entries", "stream=r_frame_rate",
                "-of", "csv=p=0",
            ])
            .arg(source)
            .output()?;

        let s = String::from_utf8_lossy(&output.stdout);
        let s = s.trim();

        let fps = if let Some((num, den)) = s.split_once('/') {
            let n: f64 = num.parse().unwrap_or(30.0);
            let d: f64 = den.parse().unwrap_or(1.0);
            if d > 0.0 { n / d } else { 30.0 }
        } else {
            s.parse().unwrap_or(30.0)
        };

        Ok(fps.min(60.0))
    }

    fn spawn_ffmpeg(
        source: &str,
        tw: u16,
        th: u16,
        fps: f64,
        start: f64,
    ) -> Result<Child, Box<dyn std::error::Error>> {
        let fps_str = format!("{fps:.2}");
        let size = format!("{}x{}", tw, th);

        let mut cmd = Command::new("ffmpeg");

        if start > 0.0 {
            cmd.args(["-ss", &format!("{start:.3}")]);
        }

        let child = cmd
            .arg("-i")
            .arg(source)
            .args([
                "-vf",
                &format!("scale={size}:flags=fast_bilinear,fps={fps_str}"),
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgb24",
                "-v",
                "quiet",
                "-nostats",
                "-",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .stdin(Stdio::null())
            .spawn()?;

        Ok(child)
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
