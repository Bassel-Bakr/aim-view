//! yt-dlp, for recordings added from a link (library/links.rs): found as ffmpeg is (ffmpeg.rs), the PATH's when it
//! runs, else the official release from GitHub, downloaded once into the tools folder beside ffmpeg's. It reads a
//! link's title and qualities (`-J`, nothing downloaded), and downloads the chosen quality as one MP4 file, merged
//! with the ffmpeg the review uses.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};

use ffmpeg_sidecar::download::{FfmpegDownloadProgressEvent, download_ffmpeg_package_with_progress};
use serde::{Deserialize, Serialize};

use crate::config::Ffmpeg;

/// One download of yt-dlp at a time.
static INSTALLING: Mutex<()> = Mutex::new(());
/// A stand-in for yt-dlp (the tests'); None: the PATH's or the tools folder's.
static STAND_IN: Mutex<Option<PathBuf>> = Mutex::new(None);
/// How long yt-dlp waits on the network before it gives up.
const SOCKET_TIMEOUT_S: &str = "15";

/// What a link offers: its title, its length in seconds, when it was uploaded, and the qualities to choose from, best
/// first.
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct LinkInfo {
    pub title: String,
    pub duration: Option<f64>,
    #[serde(skip)]
    pub timestamp: Option<f64>,
    #[serde(skip)]
    pub upload_date: Option<String>,
    pub formats: Vec<Choice>,
}

/// A quality to download: yt-dlp's format id, its frame size, frame rate, video codec, and size in bytes (with the
/// best audio) where yt-dlp knows it.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, rename = "LinkFormat"))]
pub struct Choice {
    pub id: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<f64>,
    pub codec: Option<String>,
    pub size: Option<f64>,
    /// It has its own sound: downloaded alone, not with the best audio.
    #[serde(skip)]
    pub audio: bool,
}

#[derive(Deserialize)]
struct RawInfo {
    title: Option<String>,
    duration: Option<f64>,
    timestamp: Option<f64>,
    upload_date: Option<String>,
    #[serde(default)]
    formats: Vec<RawFormat>,
}

#[derive(Clone, Deserialize)]
struct RawFormat {
    format_id: Option<String>,
    vcodec: Option<String>,
    acodec: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    fps: Option<f64>,
    filesize: Option<f64>,
    filesize_approx: Option<f64>,
    dynamic_range: Option<String>,
}

impl RawFormat {
    fn size(&self) -> Option<f64> {
        self.filesize.or(self.filesize_approx)
    }
    /// Video, or a file of unknown kind (a plain video file's link); not sound alone.
    fn has_video(&self) -> bool {
        match self.vcodec.as_deref() {
            Some(v) => v != "none",
            None => self.height.is_some() || self.acodec.is_none(),
        }
    }
    fn has_audio(&self) -> bool {
        self.acodec.as_deref().is_some_and(|a| a != "none")
    }
}

/// Where the tools folder is: beside ffmpeg's download folder, else in the data folder.
pub fn tools_folder(ffmpeg: &Ffmpeg, data: &Path) -> PathBuf {
    match ffmpeg {
        Ffmpeg::Download(f) => f.parent().unwrap_or(data).join("yt-dlp"),
        _ => data.join("yt-dlp"),
    }
}

/// Uses `program` in place of yt-dlp (the tests' stand-in); None goes back to finding it.
pub fn set_stand_in(program: Option<PathBuf>) {
    *STAND_IN.lock().unwrap_or_else(|e| e.into_inner()) = program;
}

fn stand_in() -> Option<PathBuf> {
    STAND_IN.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// A command without a console window of its own.
fn command(program: &Path) -> Command {
    let mut c = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c
}

fn runs(program: &Path) -> bool {
    let mut c = command(program);
    c.arg("--version").stdout(Stdio::null()).stderr(Stdio::null());
    c.status().is_ok_and(|s| s.success())
}

/// Whether the PATH has a yt-dlp that runs: asked once a process.
fn on_path() -> bool {
    static FOUND: OnceLock<bool> = OnceLock::new();
    *FOUND.get_or_init(|| runs(Path::new("yt-dlp")))
}

/// The release this computer runs.
fn release() -> Result<String, String> {
    let name = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "yt-dlp.exe",
        ("windows", "x86") => "yt-dlp_x86.exe",
        ("windows", "aarch64") => "yt-dlp_arm64.exe",
        ("linux", "x86_64") => "yt-dlp_linux",
        ("linux", "aarch64") => "yt-dlp_linux_aarch64",
        ("macos", _) => "yt-dlp_macos",
        (os, arch) => return Err(format!("no yt-dlp release for {os} on {arch}: put yt-dlp on the PATH")),
    };
    Ok(format!("https://github.com/yt-dlp/yt-dlp/releases/latest/download/{name}"))
}

/// yt-dlp: the stand-in, the PATH's, or the tools folder's, downloaded there the first time (`progress` hears the
/// megabytes downloaded, of how many).
pub fn ensure(tools: &Path, progress: impl Fn(usize, usize)) -> Result<PathBuf, String> {
    if let Some(p) = stand_in() {
        return Ok(p);
    }
    if on_path() {
        return Ok(PathBuf::from("yt-dlp"));
    }
    let url = release()?;
    let file = tools.join(url.rsplit('/').next().unwrap_or("yt-dlp"));
    let _one = INSTALLING.lock().unwrap_or_else(|e| e.into_inner());
    if file.is_file() && runs(&file) {
        return Ok(file);
    }
    std::fs::create_dir_all(tools).map_err(|e| format!("yt-dlp's folder could not be made: {e}"))?;
    download_ffmpeg_package_with_progress(&url, tools, |event| {
        if let FfmpegDownloadProgressEvent::Downloading { total_bytes, downloaded_bytes } = event {
            progress((downloaded_bytes >> 20) as usize, (total_bytes >> 20).max(1) as usize);
        }
    })
    .map_err(|e| format!("yt-dlp could not be downloaded: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    if !runs(&file) {
        let _ = std::fs::remove_file(&file);
        return Err("the yt-dlp downloaded does not run".into());
    }
    Ok(file)
}

/// yt-dlp's reason for a failure, in plain words: its last error line without "ERROR:", the site's name and the
/// video's id.
pub fn reason(stderr: &str) -> String {
    let line = stderr.lines().rev().find(|l| l.trim_start().starts_with("ERROR:")).map(str::trim);
    let Some(line) = line else {
        let last = stderr.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
        return if last.is_empty() { "yt-dlp could not read it".into() } else { last.to_string() };
    };
    let mut text = line.trim_start_matches("ERROR:").trim();
    // "[youtube] dQw4w9WgXcQ: Private video": the site, then the video's id
    if let Some(rest) = text.strip_prefix('[').and_then(|r| r.split_once("] ")).map(|(_, r)| r) {
        text = rest.split_once(": ").filter(|(id, _)| !id.contains(' ')).map_or(rest, |(_, r)| r);
    }
    text.trim().to_string()
}

/// A video codec's name as people know it ("avc1.640028": H.264).
fn codec_name(vcodec: &str) -> String {
    let c = vcodec.to_ascii_lowercase();
    let known = [
        ("av01", "AV1"),
        ("av1", "AV1"),
        ("vp09", "VP9"),
        ("vp9", "VP9"),
        ("vp8", "VP8"),
        ("avc", "H.264"),
        ("h264", "H.264"),
        ("hev", "H.265"),
        ("hvc", "H.265"),
        ("h265", "H.265"),
    ];
    known
        .iter()
        .find(|(prefix, _)| c.starts_with(prefix))
        .map_or_else(|| vcodec.split('.').next().unwrap_or(vcodec).to_uppercase(), |(_, name)| name.to_string())
}

/// The qualities to choose from, best first (the most pixels, then the highest frame rate): one for each frame size
/// and frame rate, the one yt-dlp ranks best (its formats come worst first), in standard range when there is one.
fn choices(formats: &[RawFormat]) -> Vec<Choice> {
    let audio = formats.iter().rev().find(|f| !f.has_video() && f.has_audio()).and_then(RawFormat::size);
    let mut best: Vec<RawFormat> = Vec::new();
    for f in formats.iter().filter(|f| f.has_video() && f.format_id.is_some()) {
        let key = |g: &RawFormat| (g.width, g.height, g.fps.map(|x| x.round() as i64));
        let sdr = |g: &RawFormat| g.dynamic_range.as_deref().is_none_or(|d| d == "SDR");
        match best.iter_mut().find(|g| key(g) == key(f)) {
            // a later one ranks higher, unless it is HDR in place of a standard one
            Some(g) if sdr(f) || !sdr(g) => *g = f.clone(),
            Some(_) => {}
            None => best.push(f.clone()),
        }
    }
    let mut out: Vec<Choice> = best
        .into_iter()
        .map(|f| Choice {
            id: f.format_id.clone().unwrap_or_default(),
            width: f.width,
            height: f.height,
            fps: f.fps,
            codec: f.vcodec.as_deref().map(codec_name),
            size: f.size().map(|v| v + if f.has_audio() { 0.0 } else { audio.unwrap_or(0.0) }),
            audio: f.has_audio(),
        })
        .collect();
    let pixels = |c: &Choice| u64::from(c.width.unwrap_or(0)) * u64::from(c.height.unwrap_or(0)) + u64::from(c.height.unwrap_or(0));
    out.sort_by(|a, b| pixels(b).cmp(&pixels(a)).then(b.fps.unwrap_or(0.0).total_cmp(&a.fps.unwrap_or(0.0))));
    out
}

/// yt-dlp's output on failure as an error in plain words.
fn failed(stderr: &[u8]) -> String {
    reason(&String::from_utf8_lossy(stderr))
}

/// A link's title, length and qualities, from yt-dlp (nothing downloaded).
pub fn info(program: &Path, url: &str) -> Result<LinkInfo, String> {
    let out = command(program)
        .args(["-J", "--no-playlist", "--ignore-config", "--no-warnings", "--encoding", "utf-8"])
        .args(["--socket-timeout", SOCKET_TIMEOUT_S, "--", url])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("yt-dlp could not start: {e}"))?;
    if !out.status.success() {
        return Err(failed(&out.stderr));
    }
    parse_info(&out.stdout)
}

fn parse_info(json: &[u8]) -> Result<LinkInfo, String> {
    let raw: RawInfo = serde_json::from_slice(json).map_err(|e| format!("yt-dlp's answer could not be read: {e}"))?;
    Ok(LinkInfo {
        title: raw.title.unwrap_or_default(),
        duration: raw.duration,
        timestamp: raw.timestamp,
        upload_date: raw.upload_date,
        formats: choices(&raw.formats),
    })
}

/// The format yt-dlp downloads: the chosen one, with the best audio when it has none; with none chosen, the best.
pub fn format_spec(chosen: Option<&str>, formats: &[Choice]) -> String {
    match chosen {
        None => "bv*+ba/b".into(),
        Some(id) => match formats.iter().find(|c| c.id == id) {
            Some(c) if c.audio => id.to_string(),
            _ => format!("{id}+ba/{id}"),
        },
    }
}

/// One line of yt-dlp's progress (the template `progress_template` gives): the format, the bytes done, and of how
/// many where known.
fn progress_line(line: &str) -> Option<(String, f64, Option<f64>)> {
    let mut parts = line.strip_prefix("aimview ")?.split(' ');
    let num = |s: Option<&str>| s.and_then(|v| v.parse::<f64>().ok());
    let id = parts.next()?.to_string();
    let done = num(parts.next())?;
    let total = num(parts.next()).or(num(parts.next()));
    Some((id, done, total))
}

/// Downloads `url` in the format `spec` into `folder` as video.mp4 (merged, or remuxed, with the ffmpeg at
/// `ffmpeg`, None: the PATH's). `progress` hears the megabytes done, of how many (each part's total, once known).
pub fn download(
    program: &Path,
    url: &str,
    spec: &str,
    folder: &Path,
    ffmpeg: Option<&Path>,
    progress: impl Fn(usize, usize),
) -> Result<PathBuf, String> {
    let mut c = command(program);
    c.args(["--no-playlist", "--ignore-config", "--no-warnings", "--encoding", "utf-8", "--newline", "--progress"])
        .args(["--no-mtime", "--socket-timeout", SOCKET_TIMEOUT_S, "-f", spec])
        .args(["--merge-output-format", "mp4", "--remux-video", "mp4"])
        .args(["--progress-template", "download:aimview %(info.format_id)s %(progress.downloaded_bytes)s %(progress.total_bytes)s %(progress.total_bytes_estimate)s"]);
    if let Some(f) = ffmpeg {
        c.arg("--ffmpeg-location").arg(f);
    }
    c.arg("-o").arg(folder.join("video.%(ext)s")).args(["--", url]);
    let mut child = c
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("yt-dlp could not start: {e}"))?;
    // its errors are read beside its progress, so neither pipe fills up
    let mut err = child.stderr.take();
    let errors = std::thread::spawn(move || {
        let mut s = Vec::new();
        if let Some(e) = err.as_mut() {
            let _ = e.read_to_end(&mut s);
        }
        s
    });
    let mut parts: Vec<(String, f64, f64)> = Vec::new();
    for line in BufReader::new(child.stdout.take().ok_or("yt-dlp gave no output")?).lines().map_while(Result::ok) {
        let Some((id, done, total)) = progress_line(line.trim()) else { continue };
        match parts.iter_mut().find(|p| p.0 == id) {
            Some(p) => (p.1, p.2) = (done, total.unwrap_or(p.2).max(done)),
            None => parts.push((id, done, total.unwrap_or(done).max(done))),
        }
        let (done, total) = parts.iter().fold((0.0, 0.0), |(d, t), p| (d + p.1, t + p.2));
        progress((done as u64 >> 20) as usize, ((total as u64 >> 20) as usize).max(1));
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    let stderr = errors.join().unwrap_or_default();
    if !status.success() {
        return Err(failed(&stderr));
    }
    let file = folder.join("video.mp4");
    if !file.is_file() {
        return Err("yt-dlp made no MP4 file".into());
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_reasons() {
        let err = "WARNING: x\nERROR: [youtube] dQw4w9WgXcQ: Private video. Sign in if you've been granted access\n";
        assert_eq!(reason(err), "Private video. Sign in if you've been granted access");
        assert_eq!(reason("ERROR: Unsupported URL: https://example.com/"), "Unsupported URL: https://example.com/");
        let net = "ERROR: [generic] Unable to download webpage: <urlopen error [Errno 11001] getaddrinfo failed>";
        assert_eq!(reason(net), "Unable to download webpage: <urlopen error [Errno 11001] getaddrinfo failed>");
        assert_eq!(reason(""), "yt-dlp could not read it");
    }

    #[test]
    fn qualities_best_first_one_per_size_and_rate() {
        let json = br#"{"title": "A run", "duration": 42.5, "upload_date": "20261001", "formats": [
            {"format_id": "sb0", "vcodec": "none", "acodec": "none", "ext": "mhtml"},
            {"format_id": "140", "vcodec": "none", "acodec": "mp4a.40.2", "filesize": 1048576},
            {"format_id": "18", "vcodec": "avc1.42001E", "acodec": "mp4a.40.2", "width": 640, "height": 360, "fps": 30, "filesize": 5000000},
            {"format_id": "299", "vcodec": "avc1.64002a", "acodec": "none", "width": 1920, "height": 1080, "fps": 60, "filesize": 20971520},
            {"format_id": "303", "vcodec": "vp09.00.41.08", "acodec": "none", "width": 1920, "height": 1080, "fps": 60, "filesize_approx": 10485760},
            {"format_id": "337", "vcodec": "vp09.02.51.10", "acodec": "none", "width": 1920, "height": 1080, "fps": 60, "dynamic_range": "HDR10"},
            {"format_id": "298", "vcodec": "avc1.4d4020", "acodec": "none", "width": 1280, "height": 720, "fps": 60},
            {"format_id": "136", "vcodec": "avc1.4d401f", "acodec": "none", "width": 1280, "height": 720, "fps": 30},
            {"format_id": "400", "vcodec": "av01.0.12M.08", "acodec": "none", "width": 2560, "height": 1440, "fps": 59.94}
        ]}"#;
        let info = parse_info(json).unwrap();
        assert_eq!(info.title, "A run");
        let ids: Vec<&str> = info.formats.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["400", "303", "298", "136", "18"]);
        assert_eq!(info.formats[0].codec.as_deref(), Some("AV1"));
        assert_eq!(info.formats[1].codec.as_deref(), Some("VP9"));
        // the best audio's size is added to a video without sound
        assert_eq!(info.formats[1].size, Some(11534336.0));
        assert_eq!(info.formats[4].size, Some(5000000.0));
        assert_eq!(format_spec(Some("303"), &info.formats), "303+ba/303");
        assert_eq!(format_spec(Some("18"), &info.formats), "18");
        assert_eq!(format_spec(None, &info.formats), "bv*+ba/b");
        // a plain video file: one format, nothing to choose
        let file = parse_info(br#"{"title": "clip", "formats": [{"format_id": "mp4", "ext": "mp4", "vcodec": null}]}"#).unwrap();
        assert_eq!(file.formats.len(), 1);
    }

    #[test]
    fn progress_lines() {
        assert_eq!(progress_line("aimview 299 1024 2048 NA"), Some(("299".into(), 1024.0, Some(2048.0))));
        assert_eq!(progress_line("aimview 140 10 NA 99.5"), Some(("140".into(), 10.0, Some(99.5))));
        assert_eq!(progress_line("[download] Destination: x"), None);
    }
}
