//! The review service (aimview-service) behind the HTTP side: the settings as its `Config`, each call as its
//! `ApiRequest`.

use std::path::Path;
use std::sync::Arc;

use aimview_service::{ApiRequest, Config, Ffmpeg, Layout, Library};

use crate::config::{Device, FfmpegChoice, Settings};
use crate::http::{Api, Call, Reply};

struct Service(Arc<Library>);

impl Api for Service {
    fn handle(&self, call: &Call) -> Reply {
        let request = ApiRequest {
            method: &call.method,
            path_and_query: &call.path_and_query,
            range: call.range.as_deref(),
            body: &call.body,
        };
        let r = aimview_service::handle(&self.0, &request);
        Reply { status: r.status, headers: r.headers, body: r.body }
    }
}

/// Whether a folder holds ffmpeg and ffprobe.
fn has_ffmpeg(folder: &Path) -> bool {
    ["ffmpeg", "ffprobe"].iter().all(|p| folder.join(format!("{p}{}", std::env::consts::EXE_SUFFIX)).is_file())
}

/// The service's settings: python/server.py's layout in the data folder.
fn config(s: &Settings) -> Config {
    Config {
        data: s.data.clone(),
        layout: Layout::Python,
        vods: s.vods.clone(),
        stats: s.stats.clone(),
        scenarios: s.scenarios.clone(),
        models: s.models.clone(),
        device: match s.device {
            Device::Auto => aimview_service::Device::Auto,
            Device::DirectMl => aimview_service::Device::DirectMl,
            Device::Cuda => aimview_service::Device::Cuda,
            Device::Cpu => aimview_service::Device::Cpu,
        },
        // a folder's own ffmpeg is used as it is; an empty folder gets the download
        ffmpeg: match &s.ffmpeg {
            FfmpegChoice::Path => Ffmpeg::Path,
            FfmpegChoice::Folder(f) if has_ffmpeg(f) => Ffmpeg::Folder(f.clone()),
            FfmpegChoice::Folder(f) => Ffmpeg::Download(f.clone()),
        },
    }
}

/// The library the settings describe, as the API.
pub fn open(s: &Settings) -> Result<Arc<dyn Api>, String> {
    if matches!(s.device, Device::Cuda) && !cfg!(feature = "cuda") {
        return Err("--device cuda needs a build with the cuda feature (cargo build -p aimview-server --features cuda)".into());
    }
    if matches!(s.device, Device::DirectMl) && !cfg!(windows) {
        return Err("--device directml is for Windows: use cuda or cpu".into());
    }
    let config = config(s);
    let ffmpeg = match &config.ffmpeg {
        Ffmpeg::Path => "ffmpeg: the PATH's".to_string(),
        Ffmpeg::Folder(f) => format!("ffmpeg: {}", f.display()),
        Ffmpeg::Download(f) => format!("ffmpeg: downloaded into {} before the first review", f.display()),
    };
    let lib = Library::open(config)?;
    println!("{ffmpeg}");
    Ok(Arc::new(Service(lib)))
}

/// The model and the device the reviews use, as the API tells them (/api/info).
pub fn describe(api: &dyn Api, device: Device) -> String {
    let call = Call { method: "GET".into(), path_and_query: "/api/info".into(), range: None, body: Default::default() };
    let info: serde_json::Value = serde_json::from_slice(&api.handle(&call).body).unwrap_or_default();
    let name = |key: &str| info[key].as_str().unwrap_or("?").to_string();
    let fallback = if device == Device::Auto { ", or the CPU when the GPU cannot start it" } else { "" };
    format!("detector: {} on {}{fallback} (--device {})", name("detector"), name("device"), device.flag())
}
