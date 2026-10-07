//! What a library needs to know (`Config`): where it keeps its files, the user's folders (the recordings, KovaaK's
//! stats files and scenarios), the models, the device the detector runs on and where ffmpeg comes from. The desktop app
//! fills it from its own folders; the review server from its settings (server/src/config.rs); aimview-tool from its
//! options.

use std::path::{Path, PathBuf};

/// A library's settings.
#[derive(Clone, Debug)]
pub struct Config {
    /// The data folder: everything the library writes is in it, laid out as `layout` says.
    pub data: PathBuf,
    /// How the library's files are laid out in the data folder.
    pub layout: Layout,
    /// The VODs folder: OBS's recordings, one folder per scenario. None: the folder the user chose in the app
    /// (settings.json), if any.
    pub vods: Option<PathBuf>,
    /// KovaaK's stats folder (its stats files are found there by name and time).
    pub stats: PathBuf,
    /// Folders of scenario files (.sce): the files in each, and in each of its subfolders (the workshop keeps one
    /// folder per scenario). A later folder's scenario of the same name wins.
    pub scenarios: Vec<PathBuf>,
    /// The models: the detector's _u8in exports (detector_<name>_u8in.onnx) and models.json, which is here or in the
    /// folder above (python/model/exports and python/model).
    pub models: PathBuf,
    /// The device the detector runs on until the user picks one (settings.json's `device`).
    pub device: Device,
    /// Where ffmpeg and ffprobe come from.
    pub ffmpeg: Ffmpeg,
    /// Decode and convert the frames on the GPU where the video allows it (gpu_frames.rs: Windows, 2560 x 1440 AV1 or
    /// H.264 MP4s); else ffmpeg's software decode. On by default: the reviews are the same, byte for byte.
    pub gpu_frames: bool,
}

/// Where a library keeps its files in the data folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// The desktop app's: the library's files in the data folder, each recording's folder in reviews/, and uploads/,
    /// cutoff/ and mouse/.
    App,
    /// python/retired/server.py's, with test_out/ as the data folder: the library's files and each recording's folder
    /// in vod_app/, and vod_uploads/, vod_model/hand/cutoff/ (detector training reads the labels there) and mouse/.
    Python,
}

/// The detector's device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Device {
    /// The GPU when there is one (DirectML on Windows, CUDA with the `cuda` feature), else the CPU; in the browser
    /// build WebGPU.
    Auto,
    /// DirectML: any GPU on Windows.
    DirectMl,
    /// CUDA: an NVIDIA GPU (needs the `cuda` feature).
    Cuda,
    /// The CPU, through ONNX Runtime's own provider.
    Cpu,
    /// The browser build's: the GPU through WebGPU.
    #[cfg(not(feature = "native"))]
    WebGpu,
    /// The browser build's: the CPU through WebAssembly.
    #[cfg(not(feature = "native"))]
    Wasm,
}

/// Where ffmpeg and ffprobe come from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ffmpeg {
    /// The PATH.
    Path,
    /// This folder.
    Folder(PathBuf),
    /// Downloaded into this folder the first time a review needs them (as KovOBS gets them), then used from there.
    Download(PathBuf),
}

/// The folders a layout puts the library's files in.
#[derive(Clone, Debug)]
pub struct Folders {
    /// settings.json, area_kinds.json, area_examples.jsonl and the other lists.
    pub files: PathBuf,
    /// Each recording's folder (its reviews, areas, marks).
    pub recordings: PathBuf,
    /// Videos added from the user's computer, and stats files in stats/.
    pub uploads: PathBuf,
    /// The detector labels of submitted cut-offs (train/ and checked.jsonl).
    pub cutoff: PathBuf,
    /// The raw mouse logs.
    pub mouse: PathBuf,
    /// The check folders of detector crops (crops.rs): every folder in it in the app's layout, the check_* ones in
    /// Python's (where vod_model/ holds the detector's training data beside them).
    pub crops: PathBuf,
    /// The start a folder's name needs to be a check folder.
    pub crop_prefix: &'static str,
}

impl Layout {
    /// The layout's folders in the data folder `data`.
    pub fn folders(self, data: &Path) -> Folders {
        match self {
            Layout::App => Folders {
                files: data.to_path_buf(),
                recordings: data.join("reviews"),
                uploads: data.join("uploads"),
                cutoff: data.join("cutoff"),
                mouse: data.join("mouse"),
                crops: data.join("crops"),
                crop_prefix: "",
            },
            Layout::Python => Folders {
                files: data.join("vod_app"),
                recordings: data.join("vod_app"),
                uploads: data.join("vod_uploads"),
                cutoff: data.join("vod_model").join("hand").join("cutoff"),
                mouse: data.join("mouse"),
                crops: data.join("vod_model"),
                crop_prefix: "check_",
            },
        }
    }
}

impl Device {
    /// A device by its name ("directml", "cuda", "cpu"; in the browser build "webgpu", "wasm"); none for another name.
    pub fn from_name(name: &str) -> Option<Device> {
        match name {
            "directml" => Some(Device::DirectMl),
            "cuda" => Some(Device::Cuda),
            "cpu" => Some(Device::Cpu),
            #[cfg(not(feature = "native"))]
            "webgpu" => Some(Device::WebGpu),
            #[cfg(not(feature = "native"))]
            "wasm" => Some(Device::Wasm),
            _ => None,
        }
    }

    /// The devices the browser build can run the detector on (the page runs it): the GPU through WebGPU, the CPU
    /// through WebAssembly.
    #[cfg(not(feature = "native"))]
    pub fn built() -> Vec<Device> {
        vec![Device::WebGpu, Device::Wasm]
    }

    /// The devices this build can run the detector on: the GPU it has a provider for (DirectML on Windows, CUDA with
    /// the `cuda` feature), and the CPU.
    #[cfg(feature = "native")]
    pub fn built() -> Vec<Device> {
        let mut out = Vec::new();
        if cfg!(windows) {
            out.push(Device::DirectMl);
        }
        if cfg!(feature = "cuda") {
            out.push(Device::Cuda);
        }
        out.push(Device::Cpu);
        out
    }

    /// The device's name as the API gives it ("webgpu" or "wasm"; `Auto` is WebGPU).
    #[cfg(not(feature = "native"))]
    pub fn name(self) -> &'static str {
        match self {
            Device::Auto | Device::WebGpu => "webgpu",
            Device::Wasm => "wasm",
            Device::DirectMl => "directml",
            Device::Cuda => "cuda",
            Device::Cpu => "cpu",
        }
    }

    /// The device's name as the API gives it (python/retired/server.py: "cuda" or "cpu"; the desktop app:
    /// "directml"). `Auto` is named for the GPU this build would try, else "cpu".
    #[cfg(feature = "native")]
    pub fn name(self) -> &'static str {
        match self {
            Device::Auto if cfg!(windows) => "directml",
            Device::Auto if cfg!(feature = "cuda") => "cuda",
            Device::Auto | Device::Cpu => "cpu",
            Device::DirectMl => "directml",
            Device::Cuda => "cuda",
        }
    }
}

impl Config {
    /// A library in `data` with the models in `models`: no VODs folder until the user chooses one, KovaaK's folders
    /// where Steam puts them, the detector on the GPU when there is one, ffmpeg from the PATH.
    pub fn new(data: PathBuf, layout: Layout, models: PathBuf) -> Config {
        let (stats, scenarios) = kovaak_folders();
        Config {
            data,
            layout,
            vods: None,
            stats,
            scenarios,
            models,
            device: Device::Auto,
            ffmpeg: Ffmpeg::Path,
            gpu_frames: true,
        }
    }

    /// The folders the library's files are in.
    pub fn folders(&self) -> Folders {
        self.layout.folders(&self.data)
    }
}

/// KovaaK's stats folder and scenario folders where this computer's Steam keeps them (aimview::local_config).
#[cfg(not(target_arch = "wasm32"))]
fn kovaak_folders() -> (PathBuf, Vec<PathBuf>) {
    let config = aimview::local_config::LocalConfig::load();
    (config.kovaak("stats").unwrap_or_default(), config.kovaak_scenarios())
}

/// None in the browser: its service is given the folders the page copied in (browser-service `service_open`).
#[cfg(target_arch = "wasm32")]
fn kovaak_folders() -> (PathBuf, Vec<PathBuf>) {
    (PathBuf::new(), Vec::new())
}
