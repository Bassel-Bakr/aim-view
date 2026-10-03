//! A recording reviewed natively, without the app: its tracks and readings written as JSON, and the time it took.
//! cargo run -p aimview-desktop --release --example track -- <video> <model _u8in.onnx> <out folder> [cap] [runs] [batch]
//! [window start] [window end] (seconds: only that part is tracked)

use std::path::PathBuf;
use std::time::Instant;

use aimview_desktop::review::{Request, TimeWindow, review};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let arg = |i: usize, default: usize| a.get(i).and_then(|v| v.parse().ok()).unwrap_or(default);
    let seconds = |i: usize| a.get(i).and_then(|v| v.parse::<f64>().ok());
    let window = seconds(7).zip(seconds(8)).map(|(start, end)| TimeWindow { start, end });
    let req = Request {
        video: a[1].clone().into(),
        model: a[2].clone().into(),
        cap: arg(4, 0),
        runs: arg(5, 2),
        batch: arg(6, 4),
        window,
    };
    let out = PathBuf::from(&a[3]);
    std::fs::create_dir_all(&out).unwrap();
    let t = Instant::now();
    let reviewed = review(&req, &|stage, done, total| eprint!("\r{stage} {done}/{total}      ")).unwrap_or_else(|e| panic!("{e}"));
    eprintln!("\nreviewed in {:.1} s with {}", t.elapsed().as_secs_f64(), reviewed.tracks.detector);
    std::fs::write(out.join("tracks.json"), serde_json::to_vec(&reviewed.tracks).unwrap()).unwrap();
    std::fs::write(out.join("readings.json"), serde_json::to_vec(&reviewed.readings).unwrap()).unwrap();
}
