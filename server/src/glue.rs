//! The review service (aimview-service) behind the HTTP side: the settings as its `Config`, each call as its
//! `ApiRequest`.
//!
//! In: the `Settings` (config.rs) and each `Call` (http.rs). Out: the service's answers as `Reply`s, and the log's
//! lines on where ffmpeg comes from and which detector runs where.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use aimview_service::{ApiRequest, Config, Ffmpeg, Layout, Library};

use crate::config::{Device, FfmpegChoice, Settings};
use crate::http::{Api, Call, Reply};

/// The library the API answers from.
struct Service(Arc<Library>);

impl Api for Service {
    /// The call as the service's request, answered by `aimview_service::handle`.
    fn handle(&self, call: &Call) -> Reply {
        let request = ApiRequest {
            method: &call.method,
            path_and_query: &call.path_and_query,
            range: call.range.as_deref(),
            body: &call.body,
            upload: call.upload.as_deref(),
        };
        let response = aimview_service::handle(&self.0, &request);
        Reply { status: response.status, headers: response.headers, body: response.body }
    }

    /// A new file in the library's uploads folder (`Library::spool`).
    fn spool(&self) -> Result<PathBuf, String> {
        self.0.spool().map_err(|failure| failure.message)
    }
}

/// Whether a folder holds ffmpeg and ffprobe.
fn has_ffmpeg(folder: &Path) -> bool {
    ["ffmpeg", "ffprobe"]
        .iter()
        .all(|program| folder.join(format!("{program}{}", std::env::consts::EXE_SUFFIX)).is_file())
}

/// The service's settings: Python's layout in the data folder (python/retired/server.py's).
fn config(settings: &Settings) -> Config {
    Config {
        data: settings.data.clone(),
        layout: Layout::Python,
        // Python's layout stays files: the training scripts read them (docs/storage-design.md)
        database: false,
        vods: settings.vods.clone(),
        stats: settings.stats.clone(),
        scenarios: settings.scenarios.clone(),
        models: settings.models.clone(),
        device: match settings.device {
            Device::Auto => aimview_service::Device::Auto,
            Device::DirectMl => aimview_service::Device::DirectMl,
            Device::Cuda => aimview_service::Device::Cuda,
            Device::Cpu => aimview_service::Device::Cpu,
        },
        gpu_frames: settings.gpu_frames,
        // a named folder's own ffmpeg is used as it is; else the PATH's when it has one, or the folder's download
        ffmpeg: match &settings.ffmpeg {
            FfmpegChoice::Path => Ffmpeg::Path,
            FfmpegChoice::Folder(folder) if has_ffmpeg(folder) => Ffmpeg::Folder(folder.clone()),
            FfmpegChoice::Folder(folder) | FfmpegChoice::Auto(folder) => Ffmpeg::Download(folder.clone()),
        },
    }
}

/// The library the settings describe, as the API.
pub fn open(settings: &Settings) -> Result<Arc<dyn Api>, String> {
    if matches!(settings.device, Device::Cuda) && !cfg!(feature = "cuda") {
        return Err(
            "--device cuda needs a build with the cuda feature (cargo build -p aimview-server --features cuda)".into()
        );
    }
    if matches!(settings.device, Device::DirectMl) && !cfg!(windows) {
        return Err("--device directml is for Windows: use cuda or cpu".into());
    }
    let library = Library::open(config(settings))?;
    println!("{}", ffmpeg_line(&library.config().ffmpeg));
    Ok(Arc::new(Service(library)))
}

/// Where the reviews' ffmpeg comes from, for the log. For the download, the PATH's is used when it has one (the
/// service asks once, here).
fn ffmpeg_line(source: &Ffmpeg) -> String {
    match source {
        Ffmpeg::Path => "ffmpeg: the PATH's".to_string(),
        Ffmpeg::Folder(folder) => format!("ffmpeg: {}", folder.display()),
        Ffmpeg::Download(folder) => {
            let program = aimview_service::ffmpeg::program("ffmpeg");
            if !program.starts_with(folder) {
                "ffmpeg: the PATH's".to_string()
            } else if program.is_file() {
                format!("ffmpeg: {} (the PATH has none)", folder.display())
            } else {
                format!("ffmpeg: downloaded into {} before the first review (the PATH has none)", folder.display())
            }
        }
    }
}

/// The model and the device the reviews use, as the API tells them (/api/info).
pub fn describe(api: &dyn Api, device: Device) -> String {
    let info: serde_json::Value = serde_json::from_slice(&api.handle(&Call::get("/api/info")).body).unwrap_or_default();
    let name = |key: &str| info[key].as_str().unwrap_or("?").to_string();
    let fallback = if device == Device::Auto { ", or the CPU when the GPU cannot start it" } else { "" };
    format!("detector: {} on {}{fallback} (--device {})", name("detector"), name("device"), device.flag())
}
