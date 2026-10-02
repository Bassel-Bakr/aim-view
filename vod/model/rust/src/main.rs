//! Runs the KovOBS detector ONNX natively and checks it against the Python (ONNX Runtime) detections.
//!
//! usage: kovobs-detect [--backend tract|ort] [--runs N] [--warmup N] [--threads N] [--expected FILE.json] [--data DIR] MODEL.onnx

mod backend;
mod decode;
mod os;

use anyhow::{Context, Result, bail};
use std::time::Instant;

const H: usize = 720;
const W: usize = 1280;
const DEFAULT_DATA: &str = r"D:\Projects\flowfix\test_out\vod_model";

fn main() -> Result<()> {
    let mut backend_name = "tract".to_string();
    let (mut runs, mut warmup) = (30usize, 3usize);
    let mut data = DEFAULT_DATA.to_string();
    let mut model = None;
    let mut threads = 1usize;
    let mut expected_path = None;
    let mut profile = false;
    let mut parallel = 0usize;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--backend" => backend_name = args.next().context("--backend needs a value")?,
            "--runs" => runs = args.next().context("--runs needs a value")?.parse()?,
            "--warmup" => warmup = args.next().context("--warmup needs a value")?.parse()?,
            "--threads" => threads = args.next().context("--threads needs a value")?.parse()?,
            "--expected" => expected_path = Some(args.next().context("--expected needs a value")?),
            "--profile" => profile = true,
            "--parallel" => parallel = args.next().context("--parallel needs a value")?.parse()?,
            "--data" => data = args.next().context("--data needs a value")?,
            _ => model = Some(a),
        }
    }
    let model = model.context("give the .onnx file as the last argument")?;

    // The frame: interleaved RGB bytes and the fixed map, as the model's NCHW float input.
    let rgb = std::fs::read(format!("{data}/bench_frame_rgb.bin"))?;
    let fixed = std::fs::read(format!("{data}/bench_frame_fixed.bin"))?;
    if rgb.len() != H * W * 3 || fixed.len() != H * W {
        bail!("frame files are not 1280 x 720");
    }
    let mut x = vec![0f32; 4 * H * W];
    for p in 0..H * W {
        for c in 0..3 {
            x[c * H * W + p] = rgb[p * 3 + c] as f32 / 255.0;
        }
        x[3 * H * W + p] = fixed[p] as f32;
    }

    let t_start = Instant::now();
    let t = Instant::now();
    let mut net = backend::load(&backend_name, &model, H, W, threads)?;
    let load = t.elapsed();
    let ws_after_load = os::working_set_mb();

    if parallel > 0 {
        net.detect(&x)?; // warm-up
        let fps = net.parallel(&x, parallel, 20)?;
        println!("parallel:   {parallel} worker threads, {fps:.1} frames/s ({:.1} ms per frame overall), peak working set {:.0} MB, {:.2} cores busy", 1000.0 / fps, os::peak_working_set_mb(), os::cpu_seconds() / t_start.elapsed().as_secs_f64());
        return Ok(());
    }
    if profile {
        net.profile(&x, 5)?;
        return Ok(());
    }
    let mut dets = Vec::new();
    for _ in 0..warmup.max(1) {
        dets = net.detect(&x)?;
    }
    let (cpu0, wall0) = (os::cpu_seconds(), Instant::now());
    let mut times = Vec::new();
    for _ in 0..runs {
        let t = Instant::now();
        dets = net.detect(&x)?;
        times.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    let wall = wall0.elapsed().as_secs_f64();
    let cores = (os::cpu_seconds() - cpu0) / wall;
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = times[times.len() / 2];
    let p90 = times[((times.len() as f64 * 0.9).ceil() as usize).min(times.len()) - 1];

    println!("model:      {model}");
    println!("backend:    {backend_name}, {threads} thread(s) requested");
    println!("detections: {}", dets.len());
    for d in &dets {
        println!("  cx {:.3}  cy {:.3}  w {:.3}  h {:.3}  score {:.3}", d[0], d[1], d[2], d[3], d[4]);
    }

    // Compare with the ONNX Runtime numbers from Python.
    let expected: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(expected_path.unwrap_or(format!("{data}/bench_expected.json")))?)?;
    let expected: Vec<Vec<f64>> = expected["detections"]
        .as_array()
        .context("no detections in bench_expected.json")?
        .iter()
        .map(|d| d.as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect())
        .collect();
    let mut ok = expected.len() == dets.len();
    let (mut max_dc, mut max_ds) = (0f64, 0f64);
    for e in &expected {
        let best = dets.iter().min_by(|a, b| {
            let da = (a[0] as f64 - e[0]).hypot(a[1] as f64 - e[1]);
            let db = (b[0] as f64 - e[0]).hypot(b[1] as f64 - e[1]);
            da.partial_cmp(&db).unwrap()
        });
        match best {
            Some(d) => {
                let dc = (d[0] as f64 - e[0]).abs().max((d[1] as f64 - e[1]).abs());
                let ds = (d[4] as f64 - e[4]).abs();
                max_dc = max_dc.max(dc);
                max_ds = max_ds.max(ds);
                if dc > 0.05 || ds > 0.01 {
                    ok = false;
                }
            }
            None => ok = false,
        }
    }
    println!(
        "match:      {} (expected {} detections, got {}; max centre error {:.4} px, max score error {:.4})",
        if ok { "YES" } else { "NO" },
        expected.len(),
        dets.len(),
        max_dc,
        max_ds
    );
    println!("load:       {:.0} ms (load + optimize + make runnable)", load.as_secs_f64() * 1000.0);
    println!("latency:    median {:.2} ms, p90 {:.2} ms (min {:.2}, max {:.2}; {} runs after {} warm-ups)", median, p90, times[0], times[times.len() - 1], runs, warmup);
    println!("cpu:        {:.2} cores busy on average during the timed runs", cores);
    println!("memory:     peak working set {:.0} MB, after load {:.0} MB", os::peak_working_set_mb(), ws_after_load);
    Ok(())
}
