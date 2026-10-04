//! A recording's frames from ffmpeg (ffmpeg.rs: the app's own copy), as Python's review decodes them
//! (python/retired/review.py: `_frames`): the video's own YUV 4:2:0 at its size, through a pipe, so the core converts
//! them to the same bytes. ffprobe gives the frames' times, the key frames and the colours. In: a video's path. Out:
//! what the video is (`VideoInfo`) and its frames, for the review (review.rs) and the area finder (finder.rs).

use std::io::Read;
use std::path::Path;
use std::process::{Child, ChildStdout, Command, Stdio};

use aimview::convert::Matrix;
use serde::Deserialize;

/// How near a whole number a frame rate is taken as that number (OBS records whole frame rates).
const WHOLE_RATE_TOLERANCE: f64 = 0.01;

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
    let mut command = Command::new(crate::ffmpeg::program(name));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// The frame times from 0 on and the key frames' times, each in order, from the packets.
fn frame_times(packets: &[ProbePacket]) -> (Vec<f64>, Vec<f64>) {
    let mut times = Vec::new();
    let mut keys = Vec::new();
    for packet in packets {
        let Some(time) = packet.pts_time.as_deref().and_then(|time| time.parse::<f64>().ok()) else { continue };
        if time < 0.0 {
            continue;
        }
        times.push(time);
        if packet.flags.as_deref().is_some_and(|flags| flags.contains('K')) {
            keys.push(time);
        }
    }
    times.sort_by(f64::total_cmp);
    keys.sort_by(f64::total_cmp);
    (times, keys)
}

/// The frame rate ffprobe gives ("60/1"), as a constant rate as the browser's review gives it: a whole number when it
/// is near one.
fn frame_rate(r_frame_rate: &str) -> f64 {
    let (numerator, denominator) = r_frame_rate.split_once('/').unwrap_or((r_frame_rate, "1"));
    let rate = numerator.parse::<f64>().unwrap_or(0.0) / denominator.parse::<f64>().unwrap_or(1.0);
    if (rate - rate.round()).abs() < WHOLE_RATE_TOLERANCE { rate.round() } else { rate }
}

/// The color matrix for ffprobe's color space (BT.601 when it names none the core knows).
fn matrix(color_space: Option<&str>) -> Matrix {
    match color_space {
        Some("bt709") => Matrix::Bt709,
        Some("fcc") => Matrix::Fcc,
        Some("smpte240m") => Matrix::Smpte240m,
        Some("bt2020nc" | "bt2020c") => Matrix::Bt2020,
        _ => Matrix::Bt601,
    }
}

/// The recording's frame times, key frames and colours, from its packets (none decoded).
pub fn probe(video: &Path) -> Result<VideoInfo, String> {
    let out = tool("ffprobe")
        .args(["-v", "error", "-select_streams", "v:0", "-show_entries", "packet=pts_time,flags"])
        .args(["-show_entries", "stream=width,height,r_frame_rate,color_space,color_range"])
        .args(["-show_entries", "format=duration", "-of", "json"])
        .arg(video)
        .output()
        .map_err(|error| format!("ffprobe could not start: {error}"))?;
    let probed: Probe =
        serde_json::from_slice(&out.stdout).map_err(|error| format!("ffprobe gave no video ({error})"))?;
    let stream = probed.streams.first().ok_or("the file has no video")?;
    let (times, keys) = frame_times(&probed.packets);
    let duration = probed.format.duration.and_then(|duration| duration.parse().ok());
    Ok(VideoInfo {
        width: stream.width,
        height: stream.height,
        fps: frame_rate(&stream.r_frame_rate),
        matrix: matrix(stream.color_space.as_deref()),
        full: stream.color_range.as_deref() == Some("pc"),
        duration: duration.unwrap_or_else(|| times.last().copied().unwrap_or(0.0)),
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
        let mut command = tool("ffmpeg");
        command.args(["-v", "error"]);
        if let Some(time) = from {
            command.args(["-ss", &format!("{time:.6}")]);
        }
        command.arg("-i").arg(video);
        if let Some(frames) = count {
            command.args(["-frames:v", &frames.to_string()]);
        }
        Frames::spawn(command.args(["-f", "rawvideo", "-pix_fmt", "yuv420p", "-"]))
    }

    fn spawn(command: &mut Command) -> Result<Frames, String> {
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .stdin(Stdio::null())
            .spawn()
            .map_err(|error| format!("ffmpeg could not start: {error}"))?;
        let out = child.stdout.take().ok_or("ffmpeg gave no output")?;
        Ok(Frames { child, out })
    }

    /// The next frame into `buf` (one frame's bytes); false when there are no more.
    pub fn next_into(&mut self, buf: &mut [u8]) -> Result<bool, String> {
        let mut got = 0;
        while got < buf.len() {
            match self.out.read(&mut buf[got..]) {
                Ok(0) => break,
                Ok(read) => got += read,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) => return Err(format!("ffmpeg's frames stopped: {error}")),
            }
        }
        match got {
            0 => Ok(false),
            whole if whole == buf.len() => Ok(true),
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
