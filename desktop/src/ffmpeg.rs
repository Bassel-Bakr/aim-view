//! ffmpeg for the review, installed the way KovOBS installs it (ffmpeg-sidecar): downloaded into the app's local data
//! folder the first time a review needs it, and unpacked there. The app does not ship it. On Windows it is BtbN's GPL
//! build, which has the dav1d AV1 decoder: gyan.dev's essentials build (KovOBS's) decodes AV1 with libaom, 2.5 times
//! slower. Without a folder set (the example), ffmpeg comes from the PATH.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use ffmpeg_sidecar::download::{
    FfmpegDownloadProgressEvent, download_ffmpeg_package_with_progress, ffmpeg_download_url, unpack_ffmpeg,
};

static FOLDER: OnceLock<PathBuf> = OnceLock::new();
/// One download at a time: two reviews that start together wait for the same one.
static INSTALLING: Mutex<()> = Mutex::new(());
/// Which build the folder holds (its download's address); another one is replaced.
const SOURCE_FILE: &str = "source.txt";

/// Where the app keeps its ffmpeg: set once, at start.
pub fn set_folder(folder: PathBuf) {
    let _ = FOLDER.set(folder);
}

/// One of ffmpeg's programs ("ffmpeg", "ffprobe"): the app's own copy.
pub fn program(name: &str) -> PathBuf {
    match FOLDER.get() {
        Some(folder) => folder.join(format!("{name}{}", std::env::consts::EXE_SUFFIX)),
        None => PathBuf::from(name),
    }
}

/// The build the app downloads.
fn download_url() -> Result<&'static str, String> {
    if cfg!(all(windows, target_arch = "x86_64")) {
        Ok("https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip")
    } else {
        ffmpeg_download_url().map_err(|e| e.to_string())
    }
}

fn installed(url: &str) -> bool {
    let Some(folder) = FOLDER.get() else { return true };
    let source = std::fs::read_to_string(folder.join(SOURCE_FILE)).unwrap_or_default();
    source.trim() == url && program("ffmpeg").is_file() && program("ffprobe").is_file()
}

/// Installs ffmpeg when the app has none yet (or an older build). `progress` hears the megabytes downloaded, of how
/// many.
pub fn ensure(progress: impl Fn(usize, usize)) -> Result<(), String> {
    let _one = INSTALLING.lock().unwrap_or_else(|e| e.into_inner());
    let url = download_url()?;
    if installed(url) {
        return Ok(());
    }
    let folder = FOLDER.get().ok_or("no folder for ffmpeg")?;
    std::fs::create_dir_all(folder).map_err(|e| format!("ffmpeg's folder could not be made: {e}"))?;
    let failed = |e: anyhow::Error| format!("ffmpeg could not be downloaded: {e}");
    let archive = download_ffmpeg_package_with_progress(url, folder, |event| {
        if let FfmpegDownloadProgressEvent::Downloading { total_bytes, downloaded_bytes } = event {
            progress((downloaded_bytes >> 20) as usize, (total_bytes >> 20).max(1) as usize);
        }
    })
    .map_err(failed)?;
    unpack_ffmpeg(&archive, folder).map_err(failed)?;
    std::fs::write(folder.join(SOURCE_FILE), url).map_err(|e| format!("ffmpeg's folder could not be written: {e}"))?;
    if !installed(url) {
        return Err("ffmpeg's download held no ffmpeg and ffprobe".into());
    }
    Ok(())
}
