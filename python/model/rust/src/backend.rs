//! The runtimes behind one small interface: take the input tensor, give back detections.

use crate::decode::decode;
use anyhow::{Result, bail};

pub trait Detector {
    fn detect(&mut self, x: &[f32]) -> Result<Vec<[f32; 5]>>;
    /// Frame-level parallelism: `threads` workers each run `frames` frames; returns frames per second (tract only).
    fn parallel(&mut self, _x: &[f32], _threads: usize, _frames: usize) -> Result<f64> {
        bail!("this backend has no parallel bench")
    }
    /// Per-node timing (tract only).
    fn profile(&mut self, _x: &[f32], _runs: usize) -> Result<()> {
        bail!("this backend has no profiler")
    }
}

pub fn load(name: &str, path: &str, h: usize, w: usize, threads: usize) -> Result<Box<dyn Detector>> {
    match name {
        "tract" => Ok(Box::new(Tract::load(path, h, w, threads)?)),
        #[cfg(feature = "ort")]
        "ort" => Ok(Box::new(OrtDetector::load(path, h, w, threads)?)),
        other => bail!("unknown backend {other} (is the feature enabled?)"),
    }
}

// ---- tract: pure Rust ----

use tract_onnx::prelude::*;

pub struct Tract {
    plan: Arc<TypedRunnableModel>,
    shape: (usize, usize, usize),
}

impl Tract {
    pub fn load(path: &str, h: usize, w: usize, threads: usize) -> Result<Self> {
        if threads > 1 {
            // Off by default in tract: matrix multiplies and element-wise ops run on a rayon pool only when asked.
            tract_linalg::multithread::set_default_executor(tract_linalg::multithread::Executor::multithread(threads));
        }
        // The fp16 file keeps symbolic h / w in its inner value_info, which clashes with the fixed input.
        let plan = tract_onnx::onnx().with_ignore_value_info(true)
            .model_for_path(path)?
            .with_input_fact(0, f32::fact([1, 4, h, w]).into())?
            .into_optimized()?
            .into_runnable()?;
        Ok(Self { plan, shape: (4, h, w) })
    }
}

impl Detector for Tract {
    fn parallel(&mut self, x: &[f32], threads: usize, frames: usize) -> Result<f64> {
        let (c, h, w) = self.shape;
        let plan = &self.plan;
        let t = std::time::Instant::now();
        std::thread::scope(|s| -> Result<()> {
            let handles: Vec<_> = (0..threads)
                .map(|_| {
                    s.spawn(move || -> Result<()> {
                        for _ in 0..frames {
                            let input = Tensor::from_shape(&[1, c, h, w], x)?;
                            let out = plan.run(tvec!(input.into()))?;
                            let (score, reg) = (out[0].try_as_plain_ram()?, out[1].try_as_plain_ram()?);
                            decode(score.as_slice::<f32>()?, reg.as_slice::<f32>()?, h / 4, w / 4);
                        }
                        Ok(())
                    })
                })
                .collect();
            for hd in handles {
                hd.join().map_err(|_| anyhow::anyhow!("worker panicked"))??;
            }
            Ok(())
        })?;
        Ok((threads * frames) as f64 / t.elapsed().as_secs_f64())
    }
    fn profile(&mut self, x: &[f32], runs: usize) -> Result<()> {
        use std::collections::HashMap;
        use std::time::Instant;
        let (c, h, w) = self.shape;
        let mut by_node: HashMap<String, (String, f64)> = HashMap::new();
        for _ in 0..runs {
            let mut state = self.plan.spawn()?;
            let input = Tensor::from_shape(&[1, c, h, w], x)?;
            state.run_plan_with_eval(tvec!(input.into()), |ctx, st, node, inputs| {
                let t = Instant::now();
                let r = tract_core::plan::eval(ctx, st, node, inputs);
                let e = by_node.entry(node.name.clone()).or_insert((node.op().name().to_string(), 0.0));
                e.1 += t.elapsed().as_secs_f64() * 1000.0 / runs as f64;
                r
            })?;
        }
        let mut nodes: Vec<_> = by_node.into_iter().collect();
        nodes.sort_by(|a, b| b.1.1.partial_cmp(&a.1.1).unwrap());
        let total: f64 = nodes.iter().map(|n| n.1.1).sum();
        let mut by_op: HashMap<String, f64> = HashMap::new();
        for n in &nodes {
            *by_op.entry(n.1.0.clone()).or_default() += n.1.1;
        }
        let mut ops: Vec<_> = by_op.into_iter().collect();
        ops.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        println!("profile ({runs} runs, total {total:.1} ms per frame), by op:");
        for (op, ms) in ops.iter().take(10) {
            println!("  {ms:8.2} ms  {op}");
        }
        println!("slowest nodes:");
        for (name, (op, ms)) in nodes.iter().take(12) {
            println!("  {ms:8.2} ms  {op:24} {name}");
        }
        Ok(())
    }

    fn detect(&mut self, x: &[f32]) -> Result<Vec<[f32; 5]>> {
        let (c, h, w) = self.shape;
        let input = Tensor::from_shape(&[1, c, h, w], x)?;
        let out = self.plan.run(tvec!(input.into()))?;
        let (score, reg) = (out[0].try_as_plain_ram()?, out[1].try_as_plain_ram()?);
        Ok(decode(score.as_slice::<f32>()?, reg.as_slice::<f32>()?, h / 4, w / 4))
    }
}

// ---- ort: ONNX Runtime bindings (cargo feature "ort") ----

#[cfg(feature = "ort")]
pub struct OrtDetector {
    session: ort::session::Session,
    shape: (usize, usize, usize),
}

#[cfg(feature = "ort")]
impl OrtDetector {
    pub fn load(path: &str, h: usize, w: usize, threads: usize) -> Result<Self> {
        let session = ort::session::Session::builder()
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .with_intra_threads(threads)
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .commit_from_file(path)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(Self { session, shape: (4, h, w) })
    }
}

#[cfg(feature = "ort")]
impl Detector for OrtDetector {
    fn detect(&mut self, x: &[f32]) -> Result<Vec<[f32; 5]>> {
        let (c, h, w) = self.shape;
        let input = ort::value::Tensor::from_array(([1usize, c, h, w], x.to_vec())).map_err(|e| anyhow::anyhow!("{e}"))?;
        let outputs = self.session.run(ort::inputs!["x" => input]).map_err(|e| anyhow::anyhow!("{e}"))?;
        let (_, score) = outputs["score"].try_extract_tensor::<f32>().map_err(|e| anyhow::anyhow!("{e}"))?;
        let (_, reg) = outputs["reg"].try_extract_tensor::<f32>().map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(decode(score, reg, h / 4, w / 4))
    }
}
