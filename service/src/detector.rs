//! The detector model on this computer, on the device the configuration says (config.rs: `Device`): ONNX Runtime with
//! DirectML (any Windows GPU), with CUDA (an NVIDIA GPU, with the `cuda` feature) or on the CPU; `Auto` tries the GPU
//! first and falls back to the CPU. It takes the _u8in export (python/model/export.py): a batch of 720p RGB frames and
//! the fixed map, as bytes. The model's settings come from its settings file beside it (`model_settings`).

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use aimview::convert::{DST_H as H, DST_W as W};
use aimview::model::{DEFAULT_THRESHOLD, ModelSettings, settings_file};
use ort::execution_providers::{CPUExecutionProvider, CUDAExecutionProvider, DirectMLExecutionProvider, ExecutionProviderDispatch};
use ort::session::Session;
use ort::session::builder::SessionBuilder;
use ort::value::{Tensor, TensorRef};

use crate::config::Device;

pub struct Detector {
    session: Session,
    fixed: Tensor<u8>,
    batch: usize,
    /// Where it runs, for the tracks' `detector` ("DirectML", "CUDA" or "CPU").
    pub device: &'static str,
}

/// One call's maps: score (batch x 1 x H/4 x W/4) and reg (batch x 4 x H/4 x W/4).
pub struct Maps {
    pub score: Vec<f32>,
    pub reg: Vec<f32>,
}

impl Detector {
    /// The model at `model`, taking `batch` frames a call, with the fixed map (W x H) for each of them, on `device`.
    pub fn new(model: &Path, batch: usize, fixed: &[u8], device: Device) -> Result<Detector, String> {
        let sized = |b: SessionBuilder| -> ort::Result<SessionBuilder> {
            b.with_dimension_override("n", batch as i64)?
                .with_dimension_override("h", H as i64)?
                .with_dimension_override("w", W as i64)
        };
        let on_gpu = |provider: ExecutionProviderDispatch| {
            Session::builder()
                .and_then(|b| b.with_execution_providers([provider.error_on_failure()]))
                // a GPU runs one node at a time, with no memory pattern (DirectML needs it so)
                .and_then(|b| b.with_parallel_execution(false))
                .and_then(|b| b.with_memory_pattern(false))
                .and_then(sized)
                .and_then(|b| b.commit_from_file(model))
        };
        let on_cpu = || {
            let threads = std::thread::available_parallelism().map_or(4, |n| n.get().min(8));
            Session::builder()
                .and_then(|b| b.with_execution_providers([CPUExecutionProvider::default().build()]))
                .and_then(|b| b.with_intra_threads(threads))
                .and_then(sized)
                .and_then(|b| b.commit_from_file(model))
        };
        let directml = || on_gpu(DirectMLExecutionProvider::default().build());
        let cuda = || on_gpu(CUDAExecutionProvider::default().build());
        let failed = |on: &str, e: ort::Error| format!("the detector could not load {} on {on}: {e}", model.display());
        let (session, device) = match device {
            Device::DirectMl => (directml().map_err(|e| failed("DirectML", e))?, "DirectML"),
            Device::Cuda => (cuda().map_err(|e| failed("CUDA", e))?, "CUDA"),
            Device::Cpu => (on_cpu().map_err(|e| failed("the CPU", e))?, "CPU"),
            Device::Auto => {
                let gpu = if cfg!(windows) { directml().map(|s| (s, "DirectML")) } else { cuda().map(|s| (s, "CUDA")) };
                match gpu {
                    Ok(found) => found,
                    Err(_) => (on_cpu().map_err(|e| failed("the CPU", e))?, "CPU"),
                }
            }
        };
        let all = fixed.repeat(batch);
        let fixed = Tensor::from_array(([batch, H, W], all)).map_err(|e| e.to_string())?;
        Ok(Detector { session, fixed, batch, device })
    }

    /// One call on a whole batch of RGB frames (batch x H x W x 3 bytes).
    pub fn run(&mut self, rgb: &[u8]) -> Result<Maps, String> {
        let rgb = TensorRef::from_array_view(([self.batch, H, W, 3], rgb)).map_err(|e| e.to_string())?;
        let out = self
            .session
            .run(ort::inputs!["rgb" => rgb, "fixed" => &self.fixed])
            .map_err(|e| format!("the detector failed: {e}"))?;
        let map = |name: &str| -> Result<Vec<f32>, String> {
            Ok(out[name].try_extract_tensor::<f32>().map_err(|e| e.to_string())?.1.to_vec())
        };
        Ok(Maps { score: map("score")?, reg: map("reg")? })
    }
}

/// The settings file beside a model's export (detector_<name>.json: src/model.rs). A model with none gets today's
/// values, said on stderr once a model; a file that cannot be read stops the review.
pub fn model_settings(model: &Path) -> Result<ModelSettings, String> {
    let name = model.file_name().and_then(|n| n.to_str()).and_then(settings_file);
    let path = name.map_or_else(|| model.with_extension("json"), |n| model.with_file_name(n));
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            ModelSettings::from_json(&text).map_err(|e| format!("the model's settings file {}: {e}", path.display()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            static SAID: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
            if let Ok(mut said) = SAID.lock()
                && !said.contains(&path)
            {
                eprintln!(
                    "{} is missing: the detector takes today's values (threshold {DEFAULT_THRESHOLD}, no score map)",
                    path.display()
                );
                said.push(path);
            }
            Ok(ModelSettings::default())
        }
        Err(e) => Err(format!("the model's settings file {} could not be read: {e}", path.display())),
    }
}
