//! ffmpeg for the review, from where the configuration says (config.rs: `Ffmpeg`): the PATH, a folder, or installed
//! the way KovOBS installs it (ffmpeg-sidecar): downloaded into a folder the first time a review needs it, and
//! unpacked there (the desktop app does not ship it). On Windows the download is BtbN's GPL build, which has the dav1d
//! AV1 decoder: gyan.dev's essentials build (KovOBS's) decodes AV1 with libaom, 2.5 times slower. Until a library sets
//! it (the examples), ffmpeg comes from the PATH.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, RwLock};

use ffmpeg_sidecar::download::{
    FfmpegDownloadProgressEvent, download_ffmpeg_package_with_progress, ffmpeg_download_url, unpack_ffmpeg,
};

use crate::config::Ffmpeg;

/// Where this process's ffmpeg comes from.
static SOURCE: RwLock<Ffmpeg> = RwLock::new(Ffmpeg::Path);
/// One download at a time: two reviews that start together wait for the same one.
static INSTALLING: Mutex<()> = Mutex::new(());
/// Which build the folder holds (its download's address); another one is replaced.
const SOURCE_FILE: &str = "source.txt";

/// Where ffmpeg comes from, for the whole process (`Library::open` sets it).
pub fn set_source(source: Ffmpeg) {
    *SOURCE.write().unwrap_or_else(|e| e.into_inner()) = source;
}

fn source() -> Ffmpeg {
    SOURCE.read().unwrap_or_else(|e| e.into_inner()).clone()
}

/// One of ffmpeg's programs ("ffmpeg", "ffprobe").
pub fn program(name: &str) -> PathBuf {
    match source() {
        Ffmpeg::Folder(folder) | Ffmpeg::Download(folder) => folder.join(format!("{name}{}", std::env::consts::EXE_SUFFIX)),
        Ffmpeg::Path => PathBuf::from(name),
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

fn installed(folder: &Path, url: &str) -> bool {
    let source = std::fs::read_to_string(folder.join(SOURCE_FILE)).unwrap_or_default();
    source.trim() == url && program("ffmpeg").is_file() && program("ffprobe").is_file()
}

/// Installs ffmpeg when it is to be downloaded and is not yet (or an older build is). `progress` hears the megabytes
/// downloaded, of how many.
pub fn ensure(progress: impl Fn(usize, usize)) -> Result<(), String> {
    let Ffmpeg::Download(ref folder) = source() else { return Ok(()) };
    let _one = INSTALLING.lock().unwrap_or_else(|e| e.into_inner());
    let url = download_url()?;
    if installed(folder, url) {
        return Ok(());
    }
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
    if !installed(folder, url) {
        return Err("ffmpeg's download held no ffmpeg and ffprobe".into());
    }
    Ok(())
}
