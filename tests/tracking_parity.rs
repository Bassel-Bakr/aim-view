//! The tracking review (src/tracking.rs, src/review.rs) against Python's on the same tracks and camera readings:
//! tests/fixtures.py --review writes test_out/parity/<case>/review/ (tracks.json, camera.json, teal.json,
//! report.json). Everything that is not a number must be equal; numbers within 1e-9 of Python's (relative).

use std::fs;
use std::path::PathBuf;

use aimview::review::{KillTimes, VideoReadings, review_tracking};
use aimview::track::Tracks;
use aimview::tracking::CameraReading;

mod common;
use common::{Diff, compare, read};

const CASES: [&str; 5] = ["spectral", "flower", "pokeball5", "controlsphere", "aethercontrol"];

#[test]
fn tracking_review_matches_python() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_out/parity");
    let Ok(text) = fs::read_to_string(root.join("scenarios.json")) else {
        eprintln!("no scenarios.json");
        return;
    };
    let facts: serde_json::Value = serde_json::from_str(&text).unwrap();
    for case in CASES {
        let dir = root.join(case).join("review");
        let Ok(text) = fs::read_to_string(dir.join("tracks.json")) else {
            eprintln!("no {}", dir.display());
            continue;
        };
        let tracks: Tracks = serde_json::from_str(&text).unwrap();
        let want = read(&dir.join("report.json"));
        let video = want["video"].as_str().unwrap();
        let stats = want["stats"].as_str().unwrap();
        let stats_text = String::from_utf8_lossy(&fs::read(stats).unwrap()).into_owned();
        let camera: Vec<CameraReading> = serde_json::from_value(read(&dir.join("camera.json"))).unwrap();
        let teal: Vec<u32> = serde_json::from_value(read(&dir.join("teal.json"))).unwrap();
        let countdown: Vec<bool> = teal.iter().map(|&c| c >= 40).collect();
        let name = std::path::Path::new(video).file_stem().unwrap().to_string_lossy();
        let scenario = name.rsplitn(3, " - ").last().unwrap().to_lowercase();
        let limit = facts["facts"][&scenario]["limit"].as_f64();
        let readings = VideoReadings { camera: &camera, countdown: &countdown };
        let got = review_tracking(&tracks, KillTimes::Stats { name: stats, text: &stats_text }, video, limit, readings, None).unwrap();
        let mut diff = Diff::default();
        compare("report.json", &serde_json::to_value(&got).unwrap(), &want, &mut diff);
        eprintln!("{case}: {} numbers equal within 1e-9, not to the bit", diff.close);
        for w in diff.wrong.iter().take(30) {
            eprintln!("  {w}");
        }
        assert!(diff.wrong.is_empty(), "{case}: {} differences", diff.wrong.len());
    }
}
