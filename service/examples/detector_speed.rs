//! The detector alone, as the native review runs it (service/src/detector.rs): a model's _u8in export on frames of
//! noise, `batch` a call, in one session or several at once (a review runs one a part), with the time a frame. For
//! comparing models, batch sizes and devices without decoding a video.
//! cargo run -p aimview-service --release --example detector_speed -- <model _u8in.onnx> [batch] [frames] [sessions]

use std::path::{Path, PathBuf};
use std::thread;
use std::time::Instant;

use aimview::convert::{DST_H, DST_W};
use aimview_service::Device;
use aimview_service::detector::Detector;

const DEFAULT_BATCH: usize = 4;
const DEFAULT_FRAMES: usize = 2400;
const DEFAULT_SESSIONS: usize = 1;
/// Calls before the timing starts (the first ones compile the graph).
const WARM_UP_CALLS: usize = 5;
const RGB_CHANNELS: usize = 3;
/// A fixed seed for the noise, so every run sees the same frames.
const NOISE_SEED: u64 = 0x5DEE_CE66;

/// Frames of noise: the detector's work does not depend on what the frames show.
fn noise(bytes: usize) -> Vec<u8> {
    let mut state = NOISE_SEED;
    (0..bytes)
        .map(|_| {
            state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            (state >> 56) as u8
        })
        .collect()
}

/// One session's run: its calls on `frames` frames after the warm-up. Returns the device it ran on and the seconds
/// its timed calls took (loading and the warm-up left out).
fn run_session(model: &Path, batch: usize, frames: usize) -> Result<(&'static str, f64), String> {
    let fixed = vec![0u8; DST_W * DST_H];
    let mut detector = Detector::new(model, batch, &fixed, Device::Auto)?;
    let rgb = noise(batch * DST_W * DST_H * RGB_CHANNELS);
    for _ in 0..WARM_UP_CALLS {
        detector.run(&rgb, |_| ())?;
    }
    let started = Instant::now();
    for _ in 0..frames.div_ceil(batch) {
        detector.run(&rgb, |_| ())?;
    }
    Ok((detector.device, started.elapsed().as_secs_f64()))
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let model = PathBuf::from(args.get(1).ok_or("give a model's _u8in.onnx")?);
    let count = |i: usize, default: usize| args.get(i).and_then(|arg| arg.parse().ok()).unwrap_or(default);
    let (batch, frames, sessions) = (count(2, DEFAULT_BATCH), count(3, DEFAULT_FRAMES), count(4, DEFAULT_SESSIONS));
    let runs: Vec<Result<(&'static str, f64), String>> = thread::scope(|scope| {
        let runs: Vec<_> = (0..sessions).map(|_| scope.spawn(|| run_session(&model, batch, frames))).collect();
        runs.into_iter().map(|run| run.join().unwrap_or_else(|_| Err("a session failed".into()))).collect()
    });
    let runs = runs.into_iter().collect::<Result<Vec<_>, _>>()?;
    let device = runs[0].0;
    // the sessions run at once: all their frames over the slowest one's time
    let seconds = runs.iter().map(|run| run.1).fold(0.0, f64::max);
    let all = frames * sessions;
    println!(
        "{}: {all} frames, batch {batch}, {sessions} session(s) on {device}: {seconds:.2} s, {:.3} ms a frame",
        model.file_name().map_or_else(String::new, |name| name.to_string_lossy().into_owned()),
        seconds * 1000.0 / all as f64
    );
    Ok(())
}
