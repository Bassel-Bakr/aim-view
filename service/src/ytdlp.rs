//! yt-dlp, for recordings added from a link (library/links.rs): found as ffmpeg is (ffmpeg.rs), the PATH's when it
//! runs, else the official release from GitHub, downloaded once into the tools folder beside ffmpeg's.
//!
//! In: a link the user gives. Out: its title and qualities (`-J`, nothing downloaded), which the page offers to choose
//! from, and the chosen quality downloaded as one MP4 file, merged with the ffmpeg the review uses, which links.rs
//! adds to the uploads.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{ChildStderr, ChildStdout, Command, Stdio};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::thread::JoinHandle;

use ffmpeg_sidecar::download::{FfmpegDownloadProgressEvent, download_ffmpeg_package_with_progress};
use serde::{Deserialize, Serialize};

use crate::config::Ffmpeg;

/// One download of yt-dlp at a time.
static INSTALLING: Mutex<()> = Mutex::new(());
/// A stand-in for yt-dlp (the tests'); None: the PATH's or the tools folder's.
static STAND_IN: Mutex<Option<PathBuf>> = Mutex::new(None);
/// How long yt-dlp waits on the network before it gives up.
const SOCKET_TIMEOUT_S: &str = "15";
/// yt-dlp's name on the PATH, and its folder in the tools folder.
const PROGRAM: &str = "yt-dlp";
/// Where the official releases are, each file by its name.
const RELEASES_URL: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/";
/// The options every run of yt-dlp takes: one video, no user config, no warnings, UTF-8 output.
const COMMON_OPTIONS: [&str; 5] = ["--no-playlist", "--ignore-config", "--no-warnings", "--encoding", "utf-8"];
/// The format yt-dlp downloads when none is chosen: the best video with the best audio, else the best file.
const BEST_FORMAT: &str = "bv*+ba/b";
/// What a downloaded file is called in its folder (yt-dlp's template), and the MP4 it ends as.
const OUTPUT_TEMPLATE: &str = "video.%(ext)s";
const OUTPUT_FILE: &str = "video.mp4";
/// What starts each of yt-dlp's progress lines (`PROGRESS_TEMPLATE`).
const PROGRESS_PREFIX: &str = "aimview ";
/// yt-dlp's progress lines while it downloads: the part's format, its bytes done, its bytes in all and yt-dlp's
/// estimate of them ("NA" where it does not know).
const PROGRESS_TEMPLATE: &str = concat!(
    "download:aimview %(info.format_id)s %(progress.downloaded_bytes)s %(progress.total_bytes)s ",
    "%(progress.total_bytes_estimate)s"
);
/// A byte count shifted right by this many bits is in megabytes (MiB), as progress is told.
const MEGABYTE_SHIFT: u32 = 20;
/// The video codecs by their names' starts in yt-dlp's formats ("avc1.640028"), and their names as people know them.
const CODEC_NAMES: [(&str, &str); 10] = [
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
            Some(vcodec) => vcodec != "none",
            None => self.height.is_some() || self.acodec.is_none(),
        }
    }

    fn has_audio(&self) -> bool {
        self.acodec.as_deref().is_some_and(|acodec| acodec != "none")
    }

    /// Standard dynamic range, or not said.
    fn is_standard_range(&self) -> bool {
        self.dynamic_range.as_deref().is_none_or(|range| range == "SDR")
    }

    /// What one choice is offered for: the frame size and the frame rate, rounded to a whole number of frames.
    fn size_and_rate(&self) -> (Option<u32>, Option<u32>, Option<i64>) {
        (self.width, self.height, self.fps.map(|fps| fps.round() as i64))
    }

    /// The format as a choice, its size with the best audio's (`audio_size`, bytes) when it has no sound of its own.
    fn into_choice(self, audio_size: Option<f64>) -> Choice {
        let audio = self.has_audio();
        Choice {
            size: self.size().map(|size| size + if audio { 0.0 } else { audio_size.unwrap_or(0.0) }),
            codec: self.vcodec.as_deref().map(codec_name),
            id: self.format_id.unwrap_or_default(),
            width: self.width,
            height: self.height,
            fps: self.fps,
            audio,
        }
    }
}

/// Where the tools folder is: beside ffmpeg's download folder, else in the data folder.
pub fn tools_folder(ffmpeg: &Ffmpeg, data: &Path) -> PathBuf {
    match ffmpeg {
        Ffmpeg::Download(folder) => folder.parent().unwrap_or(data).join(PROGRAM),
        _ => data.join(PROGRAM),
    }
}

/// Uses `program` in place of yt-dlp (the tests' stand-in); None goes back to finding it.
pub fn set_stand_in(program: Option<PathBuf>) {
    *STAND_IN.lock().unwrap_or_else(PoisonError::into_inner) = program;
}

fn stand_in() -> Option<PathBuf> {
    STAND_IN.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

/// A command without a console window of its own.
fn command(program: &Path) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

fn runs(program: &Path) -> bool {
    let mut version = command(program);
    version.arg("--version").stdout(Stdio::null()).stderr(Stdio::null());
    version.status().is_ok_and(|status| status.success())
}

/// Whether the PATH has a yt-dlp that runs: asked once a process.
fn on_path() -> bool {
    static FOUND: OnceLock<bool> = OnceLock::new();
    *FOUND.get_or_init(|| runs(Path::new(PROGRAM)))
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
    Ok(format!("{RELEASES_URL}{name}"))
}

/// A byte count in whole megabytes, as progress is told.
fn megabytes(bytes: u64) -> usize {
    (bytes >> MEGABYTE_SHIFT) as usize
}

/// yt-dlp: the stand-in, the PATH's, or the tools folder's, downloaded there the first time (`progress` hears the
/// megabytes downloaded, of how many).
pub fn ensure(tools: &Path, progress: impl Fn(usize, usize)) -> Result<PathBuf, String> {
    if let Some(program) = stand_in() {
        return Ok(program);
    }
    if on_path() {
        return Ok(PathBuf::from(PROGRAM));
    }
    let url = release()?;
    let file = tools.join(url.rsplit('/').next().unwrap_or(PROGRAM));
    let _one = INSTALLING.lock().unwrap_or_else(PoisonError::into_inner);
    if file.is_file() && runs(&file) {
        return Ok(file);
    }
    std::fs::create_dir_all(tools).map_err(|error| format!("yt-dlp's folder could not be made: {error}"))?;
    download_ffmpeg_package_with_progress(&url, tools, |event| {
        if let FfmpegDownloadProgressEvent::Downloading { total_bytes, downloaded_bytes } = event {
            progress(megabytes(downloaded_bytes), megabytes(total_bytes).max(1));
        }
    })
    .map_err(|error| format!("yt-dlp could not be downloaded: {error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        /// Read and run for everyone, write for the owner.
        const EXECUTABLE_MODE: u32 = 0o755;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(EXECUTABLE_MODE))
            .map_err(|error| error.to_string())?;
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
    let line = stderr.lines().rev().find(|line| line.trim_start().starts_with("ERROR:")).map(str::trim);
    let Some(line) = line else {
        let last = stderr.lines().rev().find(|line| !line.trim().is_empty()).unwrap_or("").trim();
        return if last.is_empty() { "yt-dlp could not read it".into() } else { last.to_string() };
    };
    let mut text = line.trim_start_matches("ERROR:").trim();
    // "[youtube] dQw4w9WgXcQ: Private video": the site, then the video's id
    if let Some(rest) = text.strip_prefix('[').and_then(|rest| rest.split_once("] ")).map(|(_, rest)| rest) {
        text = rest.split_once(": ").filter(|(id, _)| !id.contains(' ')).map_or(rest, |(_, reason)| reason);
    }
    text.trim().to_string()
}

/// A video codec's name as people know it ("avc1.640028": H.264).
fn codec_name(vcodec: &str) -> String {
    let lower = vcodec.to_ascii_lowercase();
    CODEC_NAMES
        .iter()
        .find(|(prefix, _)| lower.starts_with(prefix))
        .map_or_else(|| vcodec.split('.').next().unwrap_or(vcodec).to_uppercase(), |(_, name)| name.to_string())
}

/// The qualities to choose from, best first (the most pixels, then the highest frame rate): one for each frame size
/// and frame rate, the one yt-dlp ranks best (its formats come worst first), in standard range when there is one.
fn choices(formats: &[RawFormat]) -> Vec<Choice> {
    let audio_size =
        formats.iter().rev().find(|format| !format.has_video() && format.has_audio()).and_then(RawFormat::size);
    let mut best: Vec<RawFormat> = Vec::new();
    for format in formats.iter().filter(|format| format.has_video() && format.format_id.is_some()) {
        match best.iter_mut().find(|kept| kept.size_and_rate() == format.size_and_rate()) {
            // a later one ranks higher, unless it is HDR in place of a standard one
            Some(kept) if format.is_standard_range() || !kept.is_standard_range() => *kept = format.clone(),
            Some(_) => {}
            None => best.push(format.clone()),
        }
    }
    let mut out: Vec<Choice> = best.into_iter().map(|format| format.into_choice(audio_size)).collect();
    out.sort_by(|a, b| pixel_rank(b).cmp(&pixel_rank(a)).then(b.fps.unwrap_or(0.0).total_cmp(&a.fps.unwrap_or(0.0))));
    out
}

/// A choice's rank by its size: its pixels, the taller one first when two have as many.
fn pixel_rank(choice: &Choice) -> u64 {
    let (width, height) = (u64::from(choice.width.unwrap_or(0)), u64::from(choice.height.unwrap_or(0)));
    width * height + height
}

/// yt-dlp's output on failure as an error in plain words.
fn failed(stderr: &[u8]) -> String {
    reason(&String::from_utf8_lossy(stderr))
}

/// A link's title, length and qualities, from yt-dlp (nothing downloaded).
pub fn info(program: &Path, url: &str) -> Result<LinkInfo, String> {
    let out = command(program)
        .arg("-J")
        .args(COMMON_OPTIONS)
        .args(["--socket-timeout", SOCKET_TIMEOUT_S, "--", url])
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("yt-dlp could not start: {error}"))?;
    if !out.status.success() {
        return Err(failed(&out.stderr));
    }
    parse_info(&out.stdout)
}

fn parse_info(json: &[u8]) -> Result<LinkInfo, String> {
    let raw: RawInfo =
        serde_json::from_slice(json).map_err(|error| format!("yt-dlp's answer could not be read: {error}"))?;
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
        None => BEST_FORMAT.into(),
        Some(id) => match formats.iter().find(|choice| choice.id == id) {
            Some(choice) if choice.audio => id.to_string(),
            _ => format!("{id}+ba/{id}"),
        },
    }
}

/// One line of yt-dlp's progress (`PROGRESS_TEMPLATE`): a part's format, its bytes done, and of how many where known
/// (yt-dlp's count, else its estimate).
#[derive(Debug, PartialEq)]
struct ProgressLine {
    format_id: String,
    done_bytes: f64,
    total_bytes: Option<f64>,
}

fn progress_line(line: &str) -> Option<ProgressLine> {
    let mut fields = line.strip_prefix(PROGRESS_PREFIX)?.split(' ');
    let bytes = |field: Option<&str>| field.and_then(|text| text.parse::<f64>().ok());
    let format_id = fields.next()?.to_string();
    let done_bytes = bytes(fields.next())?;
    let total_bytes = bytes(fields.next()).or(bytes(fields.next()));
    Some(ProgressLine { format_id, done_bytes, total_bytes })
}

/// A part of a download (video or audio) as far as it got: its format, its bytes done and in all (its bytes done
/// until yt-dlp knows).
struct DownloadPart {
    format_id: String,
    done_bytes: f64,
    total_bytes: f64,
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
    let mut child = download_command(program, url, spec, folder, ffmpeg)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("yt-dlp could not start: {error}"))?;
    // its errors are read beside its progress, so neither pipe fills up
    let errors = read_in_background(child.stderr.take());
    follow_progress(child.stdout.take().ok_or("yt-dlp gave no output")?, progress);
    let status = child.wait().map_err(|error| error.to_string())?;
    let stderr = errors.join().unwrap_or_default();
    if !status.success() {
        return Err(failed(&stderr));
    }
    let file = folder.join(OUTPUT_FILE);
    if !file.is_file() {
        return Err("yt-dlp made no MP4 file".into());
    }
    Ok(file)
}

/// yt-dlp's command line for `download`.
fn download_command(program: &Path, url: &str, spec: &str, folder: &Path, ffmpeg: Option<&Path>) -> Command {
    let mut download = command(program);
    download
        .args(COMMON_OPTIONS)
        .args(["--newline", "--progress"])
        .args(["--no-mtime", "--socket-timeout", SOCKET_TIMEOUT_S, "-f", spec])
        .args(["--merge-output-format", "mp4", "--remux-video", "mp4"])
        .args(["--progress-template", PROGRESS_TEMPLATE]);
    if let Some(location) = ffmpeg {
        download.arg("--ffmpeg-location").arg(location);
    }
    download.arg("-o").arg(folder.join(OUTPUT_TEMPLATE)).args(["--", url]);
    download
}

/// A pipe's bytes, read to its end on a thread of their own.
fn read_in_background(pipe: Option<ChildStderr>) -> JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut bytes);
        }
        bytes
    })
}

/// yt-dlp's progress lines as they come, each told to `progress` as the megabytes done of all its parts, of how many.
fn follow_progress(stdout: ChildStdout, progress: impl Fn(usize, usize)) {
    let mut parts: Vec<DownloadPart> = Vec::new();
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        let Some(ProgressLine { format_id, done_bytes, total_bytes }) = progress_line(line.trim()) else { continue };
        match parts.iter_mut().find(|part| part.format_id == format_id) {
            Some(part) => {
                (part.done_bytes, part.total_bytes) =
                    (done_bytes, total_bytes.unwrap_or(part.total_bytes).max(done_bytes));
            }
            None => parts.push(DownloadPart {
                format_id,
                done_bytes,
                total_bytes: total_bytes.unwrap_or(done_bytes).max(done_bytes),
            }),
        }
        let (done, total) =
            parts.iter().fold((0.0, 0.0), |(done, total), part| (done + part.done_bytes, total + part.total_bytes));
        progress(megabytes(done as u64), megabytes(total as u64).max(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_reasons() {
        let stderr = "WARNING: x\nERROR: [youtube] dQw4w9WgXcQ: Private video. Sign in if you've been granted access\n";
        assert_eq!(reason(stderr), "Private video. Sign in if you've been granted access");
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
            {"format_id": "18", "vcodec": "avc1.42001E", "acodec": "mp4a.40.2",
             "width": 640, "height": 360, "fps": 30, "filesize": 5000000},
            {"format_id": "299", "vcodec": "avc1.64002a", "acodec": "none",
             "width": 1920, "height": 1080, "fps": 60, "filesize": 20971520},
            {"format_id": "303", "vcodec": "vp09.00.41.08", "acodec": "none",
             "width": 1920, "height": 1080, "fps": 60, "filesize_approx": 10485760},
            {"format_id": "337", "vcodec": "vp09.02.51.10", "acodec": "none",
             "width": 1920, "height": 1080, "fps": 60, "dynamic_range": "HDR10"},
            {"format_id": "298", "vcodec": "avc1.4d4020", "acodec": "none", "width": 1280, "height": 720, "fps": 60},
            {"format_id": "136", "vcodec": "avc1.4d401f", "acodec": "none", "width": 1280, "height": 720, "fps": 30},
            {"format_id": "400", "vcodec": "av01.0.12M.08", "acodec": "none",
             "width": 2560, "height": 1440, "fps": 59.94}
        ]}"#;
        let info = parse_info(json).unwrap();
        assert_eq!(info.title, "A run");
        let ids: Vec<&str> = info.formats.iter().map(|choice| choice.id.as_str()).collect();
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
        let file = parse_info(br#"{"title": "clip", "formats": [{"format_id": "mp4", "ext": "mp4", "vcodec": null}]}"#)
            .unwrap();
        assert_eq!(file.formats.len(), 1);
    }

    #[test]
    fn progress_lines() {
        let line = |format_id: &str, done_bytes, total_bytes| {
            Some(ProgressLine { format_id: format_id.into(), done_bytes, total_bytes })
        };
        assert_eq!(progress_line("aimview 299 1024 2048 NA"), line("299", 1024.0, Some(2048.0)));
        assert_eq!(progress_line("aimview 140 10 NA 99.5"), line("140", 10.0, Some(99.5)));
        assert_eq!(progress_line("[download] Destination: x"), None);
    }
}
