//! Reviews every kept run again and writes its reports: the check that a change after the tracking (matching,
//! measures, the report) moved nothing it should not. Run it before and after the change into two folders, then
//! compare them with `bun scripts/same-json.ts <before> <after> [path=name ...]` (BENCH.md, Correctness).
//!
//!   cargo run --profile quick --example review_runs -- <out> [<root> ...] [--stats <KovaaK's stats folder>]
//!
//! A run is a folder under a root with tracks.json, readings.json, hud.json and report.json, as
//! python/model/eval_video_alone.py and build_mined.py keep them (the default roots:
//! test_out/vod_model/eval/video_alone, every model's clicking runs, and test_out/vod_model/data_mined/reviews, runs of
//! every kind). Each is reviewed as the app's report request does (service/src/report.rs): with its stats file (named
//! in its report.json) and the HUD's reading, into <out>/<root's name>/<run>/stats.json, and with neither, the video
//! alone, into .../alone.json. A run whose kept report is a tracking one (mode "track") is reviewed as tracking, over
//! the time limit that report kept.

use std::fs;
use std::path::{Path, PathBuf};

use aimview::review::review_json;
use serde_json::{Value, json};

const DEFAULT_ROOTS: [&str; 2] = ["test_out/vod_model/eval/video_alone", "test_out/vod_model/data_mined/reviews"];
const DEFAULT_STATS: &str = r"C:\Program Files (x86)\Steam\steamapps\common\FPSAimTrainer\FPSAimTrainer\stats";

/// The run folders under `root`, sorted.
fn run_folders(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut waiting = vec![root.to_path_buf()];
    while let Some(folder) = waiting.pop() {
        if folder.join("tracks.json").is_file() && folder.join("report.json").is_file() {
            found.push(folder);
            continue;
        }
        let Ok(entries) = fs::read_dir(&folder) else { continue };
        waiting.extend(entries.flatten().map(|entry| entry.path()).filter(|path| path.is_dir()));
    }
    found.sort();
    found
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// The report request for a run: with its stats file's text and the HUD's reading, or (None) with neither.
fn request(run: &Path, stats_text: Option<&str>) -> Value {
    let (kept_report, readings) = (read_json(&run.join("report.json")), read_json(&run.join("readings.json")));
    let tracking = kept_report["mode"] == "track";
    json!({
        "tracks": read_json(&run.join("tracks.json")),
        "statsText": stats_text.unwrap_or_default(),
        "video": kept_report["video"],
        "stats": if stats_text.is_some() { kept_report["stats"].clone() } else { json!("") },
        "hud": if stats_text.is_some() { read_json(&run.join("hud.json")) } else { Value::Null },
        "run": null,
        "tracking": tracking,
        "limit": if tracking { kept_report["limit"].clone() } else { Value::Null },
        "camera": readings["camera"],
        "countdown": readings["countdown"],
        "faint": null,
    })
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let stats_folder = match args.iter().position(|arg| arg == "--stats") {
        Some(at) => PathBuf::from(args.drain(at..at + 2).nth(1).expect("--stats needs a folder")),
        None => PathBuf::from(DEFAULT_STATS),
    };
    let out = PathBuf::from(args.first().expect("usage: review_runs <out> [<root> ...] [--stats <folder>]"));
    let roots: Vec<PathBuf> = if args.len() > 1 {
        args[1..].iter().map(PathBuf::from).collect()
    } else {
        DEFAULT_ROOTS.iter().map(PathBuf::from).collect()
    };
    let mut reviewed = 0;
    for root in &roots {
        for run in run_folders(root) {
            let name = read_json(&run.join("report.json"))["stats"].as_str().unwrap_or_default().to_string();
            let stats_text =
                fs::read(stats_folder.join(&name)).ok().map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
            let review =
                |stats_text: Option<&str>| review_json(&serde_json::to_vec(&request(&run, stats_text)).unwrap());
            let target = out.join(root.file_name().unwrap()).join(run.strip_prefix(root).unwrap());
            fs::create_dir_all(&target).unwrap();
            if let Some(text) = &stats_text {
                fs::write(target.join("stats.json"), review(Some(text))).unwrap();
            } else {
                eprintln!("no stats file {name}: {}", run.display());
            }
            fs::write(target.join("alone.json"), review(None)).unwrap();
            reviewed += 1;
        }
    }
    eprintln!("{reviewed} runs reviewed into {}", out.display());
}
