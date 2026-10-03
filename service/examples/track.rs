//! A recording reviewed natively, without the app: its tracks, readings and HUD reading written as JSON, the time it
//! took, and the report the core works out from them (report.json), with the stats file when one is given, else from
//! the HUD's reading or the video alone.
//! cargo run -p aimview-service --release --example track -- <video> <model _u8in.onnx> <out folder> [cap] [runs] [batch]
//! [window start] [window end] (seconds: only that part is tracked; "-" for none) [stats file] [exclude.json]

use std::path::{Path, PathBuf};
use std::time::Instant;

use aimview_service::review::{Request, TimeWindow, review};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let arg = |i: usize, default: usize| a.get(i).and_then(|v| v.parse().ok()).unwrap_or(default);
    let seconds = |i: usize| a.get(i).and_then(|v| v.parse::<f64>().ok());
    let window = seconds(7).zip(seconds(8)).map(|(start, end)| TimeWindow { start, end });
    let req = Request {
        video: a[1].clone().into(),
        model: a[2].clone().into(),
        device: aimview_service::Device::Auto,
        cap: arg(4, 0),
        runs: arg(5, 2),
        batch: arg(6, 4),
        window,
        // the areas to leave out: an exclude.json ([[x0, y0, x1, y1, kind], ...]), else KovOBS's layout
        areas: a.get(10).map_or_else(aimview_service::areas::kovobs_areas, |f| {
            serde_json::from_slice(&std::fs::read(f).expect("the areas file")).expect("an exclude.json")
        }),
    };
    let out = PathBuf::from(&a[3]);
    std::fs::create_dir_all(&out).unwrap();
    let t = Instant::now();
    let reviewed = review(&req, &|stage, done, total| eprint!("\r{stage} {done}/{total}      "), &|_| {}).unwrap_or_else(|e| panic!("{e}"));
    eprintln!("\nreviewed in {:.1} s with {}", t.elapsed().as_secs_f64(), reviewed.tracks.detector);
    let hud = reviewed.hud.as_ref().map_or("not read".into(), |h| format!("{:?}, {} kills", h.game, h.kills.len()));
    eprintln!("{} frames; the HUD: {hud}", reviewed.tracks.frames.len());
    std::fs::write(out.join("tracks.json"), serde_json::to_vec(&reviewed.tracks).unwrap()).unwrap();
    std::fs::write(out.join("readings.json"), serde_json::to_vec(&reviewed.readings).unwrap()).unwrap();
    std::fs::write(out.join("hud.json"), serde_json::to_vec(&reviewed.hud).unwrap()).unwrap();
    let stats = a.get(9).map(Path::new);
    match aimview_service::report::work_out(&out, &req.video, stats, None, None, None) {
        Ok(Some(report)) => {
            let summary = &report["summary"];
            eprintln!("report: kills from {}, {} kills", summary["info"]["source"], summary["kills"]);
            std::fs::write(out.join("report.json"), serde_json::to_vec(&report).unwrap()).unwrap();
        }
        Ok(None) => eprintln!("report: no tracks"),
        Err(e) => eprintln!("report: {e}"),
    }
}
