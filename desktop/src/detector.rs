//! The detector model on this computer: ONNX Runtime with DirectML (any Windows GPU), else on the CPU. It takes the
//! _u8in export (python/model/export.py): a batch of 720p RGB frames and the fixed map, as bytes.

use std::path::Path;

use aimview::convert::{DST_H as H, DST_W as W};
use ort::execution_providers::{CPUExecutionProvider, DirectMLExecutionProvider};
use ort::session::Session;
use ort::session::builder::SessionBuilder;
use ort::value::{Tensor, TensorRef};

pub struct Detector {
    session: Session,
    fixed: Tensor<u8>,
    batch: usize,
    /// Where it runs, for the tracks' `detector` ("DirectML" or "CPU").
    pub device: &'static str,
}

/// One call's maps: score (batch x 1 x H/4 x W/4) and reg (batch x 4 x H/4 x W/4).
pub struct Maps {
    pub score: Vec<f32>,
    pub reg: Vec<f32>,
}

impl Detector {
    /// The model at `model`, taking `batch` frames a call, with the fixed map (W x H) for each of them.
    pub fn new(model: &Path, batch: usize, fixed: &[u8]) -> Result<Detector, String> {
        let sized = |b: SessionBuilder| -> ort::Result<SessionBuilder> {
            b.with_dimension_override("n", batch as i64)?
                .with_dimension_override("h", H as i64)?
                .with_dimension_override("w", W as i64)
        };
        let gpu = Session::builder()
            .and_then(|b| b.with_execution_providers([DirectMLExecutionProvider::default().build().error_on_failure()]))
            // DirectML runs one node at a time, with no memory pattern
            .and_then(|b| b.with_parallel_execution(false))
            .and_then(|b| b.with_memory_pattern(false))
            .and_then(sized)
            .and_then(|b| b.commit_from_file(model));
        let (session, device) = match gpu {
            Ok(s) => (s, "DirectML"),
            Err(_) => {
                let threads = std::thread::available_parallelism().map_or(4, |n| n.get().min(8));
                let s = Session::builder()
                    .and_then(|b| b.with_execution_providers([CPUExecutionProvider::default().build()]))
                    .and_then(|b| b.with_intra_threads(threads))
                    .and_then(sized)
                    .and_then(|b| b.commit_from_file(model))
                    .map_err(|e| format!("the detector could not load {}: {e}", model.display()))?;
                (s, "CPU")
            }
        };
        let all: Vec<u8> = (0..batch).flat_map(|_| fixed.iter().copied()).collect();
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
