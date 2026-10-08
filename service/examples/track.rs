//! A recording reviewed natively, without the app: its tracks, readings and HUD reading written as JSON, the time it
//! took, and the report the core works out from them (report.json), with the stats file when one is given, else from
//! the HUD's reading or the video alone.
//! cargo run -p aimview-service --release --example track -- <video> <model _u8in.onnx> <out folder> [cap] [runs]
//! [batch] [window start] [window end] (seconds: only that part is tracked; "-" for none) [stats file] [exclude.json]
//! [--parts <folder>] (the review's parts kept there before they are joined, for tests/replay.rs)

use std::path::{Path, PathBuf};
use std::time::Instant;

use aimview::hud::HudReading;
use aimview_service::review::{Request, TimeWindow, review};

/// `--features dhat-heap`: every allocation counted by where it was made, written to dhat-heap.json when the review
/// ends (open it in DHAT's viewer, dh_view.html).
#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

/// The video's place on the command line (after the program's own name, 0).
const VIDEO_ARG: usize = 1;
/// The model's place.
const MODEL_ARG: usize = 2;
/// The out folder's place.
const OUT_ARG: usize = 3;
/// The target count's place.
const CAP_ARG: usize = 4;
/// The runs' place.
const RUNS_ARG: usize = 5;
/// The batch's place.
const BATCH_ARG: usize = 6;
/// The window's start's place.
const WINDOW_START_ARG: usize = 7;
/// The window's end's place.
const WINDOW_END_ARG: usize = 8;
/// The stats file's place.
const STATS_ARG: usize = 9;
/// The areas file's place.
const AREAS_ARG: usize = 10;
/// The target count when none is given: not known (0).
const DEFAULT_CAP: usize = 0;
/// The runs when none are given.
const DEFAULT_RUNS: usize = 2;
/// The frames in each detector call when none are given.
const DEFAULT_BATCH: usize = 4;

/// The review request the command line gives (`--parts` already taken out of it).
fn request(args: &[String], parts: Option<PathBuf>) -> Request {
    let count = |i: usize, default: usize| args.get(i).and_then(|arg| arg.parse().ok()).unwrap_or(default);
    let seconds = |i: usize| args.get(i).and_then(|arg| arg.parse::<f64>().ok());
    let window = seconds(WINDOW_START_ARG).zip(seconds(WINDOW_END_ARG)).map(|(start, end)| TimeWindow { start, end });
    Request {
        video: args[VIDEO_ARG].clone().into(),
        model: args[MODEL_ARG].clone().into(),
        device: aimview_service::Device::Auto,
        cap: count(CAP_ARG, DEFAULT_CAP),
        runs: count(RUNS_ARG, DEFAULT_RUNS),
        batch: count(BATCH_ARG, DEFAULT_BATCH),
        window,
        // the areas to leave out: an exclude.json ([[x0, y0, x1, y1, kind], ...]), else KovOBS's layout
        areas: args.get(AREAS_ARG).map_or_else(aimview_service::areas::kovobs_areas, |file| {
            serde_json::from_slice(&std::fs::read(file).expect("the areas file")).expect("an exclude.json")
        }),
        keep_parts: parts,
        // AIMVIEW_GPU_FRAMES=1: decode and convert on the GPU where the video allows it (gpu_frames.rs)
        gpu_frames: std::env::var("AIMVIEW_GPU_FRAMES").is_ok_and(|value| value == "1"),
        gpu_share: 1.0,
        kill_check: false,
        kind: None,
        cancel: None,
    }
}

/// Reviews the video, writes tracks.json, readings.json, hud.json and report.json into the out folder, and tells the
/// time, the frames, the HUD and the report's kills on stderr; panics when the review fails.
fn main() {
    #[cfg(feature = "dhat-heap")]
    let _heap = dhat::Profiler::new_heap();
    let mut args: Vec<String> = std::env::args().collect();
    let parts = args.iter().position(|arg| arg == "--parts");
    let parts = parts.map(|i| PathBuf::from(args.drain(i..i + 2).nth(1).expect("--parts <folder>")));
    let request = request(&args, parts);
    let out = PathBuf::from(&args[OUT_ARG]);
    std::fs::create_dir_all(&out).unwrap();
    let started = Instant::now();
    let progress = |stage: &str, done: usize, total: usize| eprint!("\r{stage} {done}/{total}      ");
    let reviewed = review(&request, &progress, &|_| {}).unwrap_or_else(|error| panic!("{error}"));
    eprintln!("\nreviewed in {:.1} s with {}", started.elapsed().as_secs_f64(), reviewed.tracks.detector);
    let hud_text = |hud: &HudReading| format!("{:?}, {} kills", hud.game, hud.kills.len());
    let hud = reviewed.hud.as_ref().map_or("not read".into(), hud_text);
    eprintln!("{} frames; the HUD: {hud}", reviewed.tracks.frames.len());
    std::fs::write(out.join("tracks.json"), serde_json::to_vec(&reviewed.tracks).unwrap()).unwrap();
    std::fs::write(out.join("readings.json"), serde_json::to_vec(&reviewed.readings).unwrap()).unwrap();
    std::fs::write(out.join("hud.json"), serde_json::to_vec(&reviewed.hud).unwrap()).unwrap();
    let stats_path = args.get(STATS_ARG).map(Path::new);
    let stats_text = stats_path.map(|path| std::fs::read(path).unwrap_or_else(|error| panic!("{error}")));
    let stats = stats_path.zip(stats_text.as_deref());
    let parts = aimview_service::store::folder_parts(&out);
    match aimview_service::report::work_out(parts, &request.video, stats, None, None, None, None) {
        Ok(Some(report)) => {
            let summary = &report["summary"];
            eprintln!("report: kills from {}, {} kills", summary["info"]["source"], summary["kills"]);
            std::fs::write(out.join("report.json"), serde_json::to_vec(&report).unwrap()).unwrap();
        }
        Ok(None) => eprintln!("report: no tracks"),
        Err(error) => eprintln!("report: {error}"),
    }
}
