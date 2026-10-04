//! A recording's frames from ffmpeg (ffmpeg.rs: the app's own copy), as Python's review decodes them
//! (python/retired/review.py: `_frames`): the video's own YUV 4:2:0 at its size, through a pipe, so the core converts
//! them to the same bytes. ffprobe gives the frames' times, the key frames and the colours.

use std::io::Read;
use std::path::Path;
use std::process::{Child, ChildStdout, Command, Stdio};

use aimview::convert::Matrix;
use serde::Deserialize;

/// What a recording is: its frames' size, rate and colours, every frame's time (from 0 on, in order: the edit list's
/// pre-roll before 0 is not shown), the key frames' times, and its duration as ffprobe gives it, in seconds.
pub struct VideoInfo {
    pub width: usize,
    pub height: usize,
    pub fps: f64,
    pub matrix: Matrix,
    pub full: bool,
    pub times: Vec<f64>,
    pub keys: Vec<f64>,
    pub duration: f64,
}

#[derive(Deserialize)]
struct Probe {
    streams: Vec<ProbeStream>,
    packets: Vec<ProbePacket>,
    #[serde(default)]
    format: ProbeFormat,
}

#[derive(Default, Deserialize)]
struct ProbeFormat {
    duration: Option<String>,
}

#[derive(Deserialize)]
struct ProbeStream {
    width: usize,
    height: usize,
    r_frame_rate: String,
    color_space: Option<String>,
    color_range: Option<String>,
}

#[derive(Deserialize)]
struct ProbePacket {
    pts_time: Option<String>,
    flags: Option<String>,
}

/// A command for one of ffmpeg's programs, without a console window of its own.
fn tool(name: &str) -> Command {
    let mut c = Command::new(crate::ffmpeg::program(name));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c
}

/// The recording's frame times, key frames and colours, from its packets (none decoded).
pub fn probe(video: &Path) -> Result<VideoInfo, String> {
    let out = tool("ffprobe")
        .args(["-v", "error", "-select_streams", "v:0", "-show_entries", "packet=pts_time,flags"])
        .args(["-show_entries", "stream=width,height,r_frame_rate,color_space,color_range"])
        .args(["-show_entries", "format=duration", "-of", "json"])
        .arg(video)
        .output()
        .map_err(|e| format!("ffprobe could not start: {e}"))?;
    let p: Probe = serde_json::from_slice(&out.stdout).map_err(|e| format!("ffprobe gave no video ({e})"))?;
    let s = p.streams.first().ok_or("the file has no video")?;
    let mut times = Vec::new();
    let mut keys = Vec::new();
    for pk in &p.packets {
        let Some(t) = pk.pts_time.as_deref().and_then(|t| t.parse::<f64>().ok()) else { continue };
        if t < 0.0 {
            continue;
        }
        times.push(t);
        if pk.flags.as_deref().is_some_and(|f| f.contains('K')) {
            keys.push(t);
        }
    }
    times.sort_by(f64::total_cmp);
    keys.sort_by(f64::total_cmp);
    let (num, den) = s.r_frame_rate.split_once('/').unwrap_or((&s.r_frame_rate, "1"));
    let rate = num.parse::<f64>().unwrap_or(0.0) / den.parse::<f64>().unwrap_or(1.0);
    // a constant rate as the browser's review gives it (OBS records whole frame rates)
    let fps = if (rate - rate.round()).abs() < 0.01 { rate.round() } else { rate };
    let matrix = match s.color_space.as_deref() {
        Some("bt709") => Matrix::Bt709,
        Some("fcc") => Matrix::Fcc,
        Some("smpte240m") => Matrix::Smpte240m,
        Some("bt2020nc" | "bt2020c") => Matrix::Bt2020,
        _ => Matrix::Bt601,
    };
    Ok(VideoInfo {
        width: s.width,
        height: s.height,
        fps,
        matrix,
        full: s.color_range.as_deref() == Some("pc"),
        duration: p.format.duration.and_then(|d| d.parse().ok()).unwrap_or_else(|| times.last().copied().unwrap_or(0.0)),
        times,
        keys,
    })
}

/// A recording's frames from an ffmpeg process, one at a time; the process ends when this is dropped.
pub struct Frames {
    child: Child,
    out: ChildStdout,
}

impl Frames {
    /// Every frame, or from a key frame's time on (ffmpeg's seek lands on the key frame itself when it is given its
    /// exact time), at most `count` of them.
    pub fn open(video: &Path, from: Option<f64>, count: Option<usize>) -> Result<Frames, String> {
        let mut c = tool("ffmpeg");
        c.args(["-v", "error"]);
        if let Some(t) = from {
            c.args(["-ss", &format!("{t:.6}")]);
        }
        c.arg("-i").arg(video);
        if let Some(n) = count {
            c.args(["-frames:v", &n.to_string()]);
        }
        Frames::spawn(c.args(["-f", "rawvideo", "-pix_fmt", "yuv420p", "-"]))
    }

    fn spawn(c: &mut Command) -> Result<Frames, String> {
        let mut child = c
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .stdin(Stdio::null())
            .spawn()
            .map_err(|e| format!("ffmpeg could not start: {e}"))?;
        let out = child.stdout.take().ok_or("ffmpeg gave no output")?;
        Ok(Frames { child, out })
    }

    /// The next frame into `buf` (one frame's bytes); false when there are no more.
    pub fn next_into(&mut self, buf: &mut [u8]) -> Result<bool, String> {
        let mut got = 0;
        while got < buf.len() {
            match self.out.read(&mut buf[got..]) {
                Ok(0) => break,
                Ok(n) => got += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return Err(format!("ffmpeg's frames stopped: {e}")),
            }
        }
        match got {
            0 => Ok(false),
            n if n == buf.len() => Ok(true),
            _ => Err("ffmpeg's last frame was cut short".into()),
        }
    }
}

impl Drop for Frames {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
