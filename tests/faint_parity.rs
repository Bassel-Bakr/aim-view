//! The faint-target cut-off (src/faint.rs, review.rs) against Python's: tests/fixtures.py --faint writes
//! test_out/parity/<case>/faint/<offset>/report.json (a tracking review with the cut-off on) and
//! test_out/parity/faint/<recording>.json (the scores, the cut and the labels of every recording the user set a
//! cut-off for). Everything that is not a number must be equal; numbers within 1e-9 of Python's (relative).

use std::fs;
use std::path::PathBuf;

use aimview::faint::{CutoffRequest, FaintSetting, cutoff_crops, faint_scores, without_faint};
use aimview::review::{KillTimes, VideoReadings, review_tracking};
use aimview::track::Tracks;
use aimview::tracking::CameraReading;

mod common;
use common::{Diff, compare, read};

const CASES: [&str; 5] = ["spectral", "flower", "pokeball5", "controlsphere", "aethercontrol"];
const OFFSETS: [&str; 3] = ["0.2", "0.3", "0.45"];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_out/parity")
}

#[test]
fn tracking_review_with_the_cut_off_matches_python() {
    let root = root();
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
        let camera: Vec<CameraReading> = serde_json::from_value(read(&dir.join("camera.json"))).unwrap();
        let teal: Vec<u32> = serde_json::from_value(read(&dir.join("teal.json"))).unwrap();
        let countdown: Vec<bool> = teal.iter().map(|&c| c >= 40).collect();
        for offset in OFFSETS {
            let path = root.join(case).join("faint").join(offset).join("report.json");
            if !path.exists() {
                eprintln!("no {}", path.display());
                continue;
            }
            let want = read(&path);
            let video = want["video"].as_str().unwrap();
            let stats = want["stats"].as_str().unwrap();
            let stats_text = String::from_utf8_lossy(&fs::read(stats).unwrap()).into_owned();
            let name = std::path::Path::new(video).file_stem().unwrap().to_string_lossy();
            let scenario = name.rsplitn(3, " - ").last().unwrap().to_lowercase();
            let limit = facts["facts"][&scenario]["limit"].as_f64();
            let readings = VideoReadings { camera: &camera, countdown: &countdown };
            let faint = FaintSetting { on: true, offset: offset.parse().unwrap() };
            let kills = KillTimes::Stats { name: stats, text: &stats_text };
            let got = review_tracking(&tracks, kills, video, limit, readings, None, Some(faint)).unwrap();
            let mut diff = Diff::default();
            let got = serde_json::to_value(&got).unwrap();
            compare("report.json", &got, &want, &mut diff);
            eprintln!(
                "{case} {offset}: cut {} ({} tracks), {} numbers equal within 1e-9, not to the bit",
                want["summary"]["faint"]["cut"], want["summary"]["faint"]["tracks"], diff.close
            );
            for w in diff.wrong.iter().take(30) {
                eprintln!("  {w}");
            }
            assert!(diff.wrong.is_empty(), "{case} {offset}: {} differences", diff.wrong.len());
        }
    }
}

#[test]
fn the_users_cut_offs_match_python() {
    let Ok(list) = fs::read_dir(root().join("faint")) else {
        eprintln!("no test_out/parity/faint");
        return;
    };
    for entry in list.flatten() {
        let want = read(&entry.path());
        let tracks: Tracks = serde_json::from_value(read(std::path::Path::new(want["tracks"].as_str().unwrap()))).unwrap();
        let near = want["near"].as_f64().unwrap();
        let offset = want["offset"].as_f64().unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();

        let sc = faint_scores(&tracks.frames, near);
        let scores: Vec<serde_json::Value> =
            sc.scores.iter().map(|t| serde_json::json!([t.id, t.score, t.frames])).collect();
        assert_eq!(serde_json::Value::from(scores), want["scores"], "{name}: scores");
        assert_eq!(sc.level, want["level"].as_f64(), "{name}: level");

        let cut = without_faint(&tracks.frames, offset, near);
        assert_eq!(cut.cut, want["cut"].as_f64(), "{name}: cut");
        assert_eq!(cut.gone as u64, want["gone"].as_u64().unwrap(), "{name}: tracks cut");
        let points: usize = cut.frames.iter().map(|f| f.t.len()).sum();
        assert_eq!(points as u64, want["points"].as_u64().unwrap(), "{name}: targets left");

        let request = CutoffRequest {
            frames: tracks.frames,
            video: want["video"].as_str().unwrap().into(),
            start: want["start"].as_i64(),
            end: want["end"].as_i64(),
            exclude: serde_json::from_value(want["exclude"].clone()).unwrap(),
            offset,
            near,
        };
        let crops = cutoff_crops(&request);
        let wanted = want["crops"].as_array().unwrap();
        assert_eq!(crops.len(), wanted.len(), "{name}: crops");
        let mut diff = Diff::default();
        for (k, (c, w)) in crops.iter().zip(wanted).enumerate() {
            assert_eq!(
                (c.frame as u64, c.x0 as u64, c.y0 as u64),
                (w["frame"].as_u64().unwrap(), w["x0"].as_u64().unwrap(), w["y0"].as_u64().unwrap()),
                "{name}: crop {k}'s frame and corner"
            );
            let boxes: Vec<[f32; 4]> = c.boxes.iter().map(|b| b.map(|v| v as f32)).collect();
            let want_boxes: Vec<[f32; 4]> = serde_json::from_value(w["boxes"].clone()).unwrap();
            assert_eq!(boxes, want_boxes, "{name}: crop {k}'s boxes (float32)");
            compare(&format!("crop {k}"), &serde_json::to_value(&c.row).unwrap(), &w["row"], &mut diff);
        }
        eprintln!(
            "{name}: {} tracks scored, cut {:?} ({} tracks), {} crops; {} numbers equal within 1e-9, not to the bit",
            sc.scores.len(),
            cut.cut,
            cut.gone,
            crops.len(),
            diff.close
        );
        for w in diff.wrong.iter().take(30) {
            eprintln!("  {w}");
        }
        assert!(diff.wrong.is_empty(), "{name}: {} differences", diff.wrong.len());
    }
}
