//! ffmpeg for the review, from where the configuration says (config.rs: `Ffmpeg`): the PATH, a folder, or found the
//! way KovOBS finds it: the PATH's ffmpeg and ffprobe when both run, else (ffmpeg-sidecar) downloaded into a folder
//! the first time a review needs it, and unpacked there (the desktop app does not ship it). On Windows the download is
//! BtbN's GPL build, which has the dav1d AV1 decoder: gyan.dev's essentials build (KovOBS's) decodes AV1 with libaom,
//! 2.5 times slower. Until a library sets it (the examples), ffmpeg comes from the PATH. In: the library's
//! configuration. Out: the programs' paths, which video.rs and links.rs run.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock, PoisonError, RwLock};

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
/// The build the app downloads on 64-bit Windows: BtbN's, with dav1d.
const WINDOWS_BUILD_URL: &str =
    "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip";
/// A byte count shifted right by this many bits is in megabytes (MiB), as progress is told.
const MEGABYTE_SHIFT: u32 = 20;
/// The player the download also unpacks: the review never runs it, and BtbN's Windows build of it is 170 MB.
const PLAYER: &str = "ffplay";

/// Where ffmpeg comes from, for the whole process (`Library::open` sets it).
pub fn set_source(source: Ffmpeg) {
    *SOURCE.write().unwrap_or_else(PoisonError::into_inner) = source;
}

fn source() -> Ffmpeg {
    SOURCE.read().unwrap_or_else(PoisonError::into_inner).clone()
}

/// Whether the PATH has an ffmpeg and an ffprobe that run: asked once a process.
fn on_path() -> bool {
    static FOUND: OnceLock<bool> = OnceLock::new();
    *FOUND.get_or_init(|| {
        let runs = |name: &str| {
            let mut command = Command::new(name);
            command.arg("-version").stdout(Stdio::null()).stderr(Stdio::null());
            command.status().is_ok_and(|status| status.success())
        };
        runs("ffmpeg") && runs("ffprobe")
    })
}

/// One of ffmpeg's programs ("ffmpeg", "ffprobe").
pub fn program(name: &str) -> PathBuf {
    match source() {
        Ffmpeg::Download(_) if on_path() => PathBuf::from(name),
        Ffmpeg::Folder(folder) | Ffmpeg::Download(folder) => {
            folder.join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
        }
        Ffmpeg::Path => PathBuf::from(name),
    }
}

/// The build the app downloads.
fn download_url() -> Result<&'static str, String> {
    if cfg!(all(windows, target_arch = "x86_64")) {
        Ok(WINDOWS_BUILD_URL)
    } else {
        ffmpeg_download_url().map_err(|error| error.to_string())
    }
}

/// The downloaded folder without ffplay (also from a folder an older version unpacked it into).
fn without_player(folder: &Path) {
    let _ = std::fs::remove_file(folder.join(format!("{PLAYER}{}", std::env::consts::EXE_SUFFIX)));
}

fn installed(folder: &Path, url: &str) -> bool {
    let source = std::fs::read_to_string(folder.join(SOURCE_FILE)).unwrap_or_default();
    source.trim() == url && program("ffmpeg").is_file() && program("ffprobe").is_file()
}

/// Installs ffmpeg when it is to be downloaded, the PATH has none, and it is not yet installed (or an older build is).
/// `progress` hears the megabytes downloaded, of how many.
pub fn ensure(progress: impl Fn(usize, usize)) -> Result<(), String> {
    let Ffmpeg::Download(ref folder) = source() else { return Ok(()) };
    if on_path() {
        return Ok(());
    }
    let _one = INSTALLING.lock().unwrap_or_else(PoisonError::into_inner);
    let url = download_url()?;
    if installed(folder, url) {
        without_player(folder);
        return Ok(());
    }
    std::fs::create_dir_all(folder).map_err(|error| format!("ffmpeg's folder could not be made: {error}"))?;
    let failed = |error: anyhow::Error| format!("ffmpeg could not be downloaded: {error}");
    let archive = download_ffmpeg_package_with_progress(url, folder, |event| {
        if let FfmpegDownloadProgressEvent::Downloading { total_bytes, downloaded_bytes } = event {
            progress((downloaded_bytes >> MEGABYTE_SHIFT) as usize, (total_bytes >> MEGABYTE_SHIFT).max(1) as usize);
        }
    })
    .map_err(failed)?;
    unpack_ffmpeg(&archive, folder).map_err(failed)?;
    without_player(folder);
    std::fs::write(folder.join(SOURCE_FILE), url)
        .map_err(|error| format!("ffmpeg's folder could not be written: {error}"))?;
    if !installed(folder, url) {
        return Err("ffmpeg's download held no ffmpeg and ffprobe".into());
    }
    Ok(())
}
