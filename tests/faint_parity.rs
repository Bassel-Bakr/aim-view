//! The faint-target cut-off (src/faint.rs, review.rs) against Python's: python/retired/tests/fixtures.py --faint writes
//! test_out/parity/<case>/faint/<offset>/report.json (a tracking review with the cut-off on) and
//! test_out/parity/faint/<recording>.json (the scores, the cut and the labels of every recording the user set a
//! cut-off for). Everything that is not a number must be equal; numbers within 1e-9 of Python's (relative).

use std::fs;
use std::path::Path;

use aimview::faint::{
    CutoffCrop, CutoffRequest, FaintSetting, TrackScore, cutoff_crops, faint_scores, without_faint,
};
use aimview::track::Tracks;
use serde_json::{Value, json};

mod common;
use common::{Diff, TrackingInputs, compare, parity_root, read};

const CASES: [&str; 5] = ["spectral", "flower", "pokeball5", "controlsphere", "aethercontrol"];
const OFFSETS: [&str; 3] = ["0.2", "0.3", "0.45"];

#[test]
fn tracking_review_with_the_cut_off_matches_python() {
    let root = parity_root();
    let Ok(text) = fs::read_to_string(root.join("scenarios.json")) else {
        eprintln!("no scenarios.json");
        return;
    };
    let facts: Value = serde_json::from_str(&text).unwrap();
    for case in CASES {
        let dir = root.join(case).join("review");
        let Some(inputs) = TrackingInputs::read(&dir) else {
            eprintln!("no {}", dir.display());
            continue;
        };
        for offset in OFFSETS {
            let path = root.join(case).join("faint").join(offset).join("report.json");
            if !path.exists() {
                eprintln!("no {}", path.display());
                continue;
            }
            let want = read(&path);
            let faint = FaintSetting { on: true, offset: offset.parse().unwrap() };
            let got = inputs.review(&want, &facts, Some(faint));
            let mut diff = Diff::default();
            compare("report.json", &got, &want, &mut diff);
            let label = format!("{case} {offset}");
            let python_cut = &want["summary"]["faint"];
            let (cut, tracks, close) = (&python_cut["cut"], &python_cut["tracks"], diff.close);
            eprintln!("{label}: cut {cut} ({tracks} tracks), {close} numbers equal within 1e-9, not to the bit");
            diff.assert_none(&label);
        }
    }
}

/// The crops the core picks for a submit's labels against Python's: each one's frame and corner, its boxes as float32,
/// and its row of numbers (into `diff`).
fn compare_crops(name: &str, crops: &[CutoffCrop], wanted: &[Value], diff: &mut Diff) {
    assert_eq!(crops.len(), wanted.len(), "{name}: crops");
    for (i, (crop, want)) in crops.iter().zip(wanted).enumerate() {
        let corner = |key: &str| want[key].as_u64().unwrap();
        assert_eq!(
            (crop.frame as u64, crop.x0 as u64, crop.y0 as u64),
            (corner("frame"), corner("x0"), corner("y0")),
            "{name}: crop {i}'s frame and corner"
        );
        let boxes: Vec<[f32; 4]> = crop.boxes.iter().map(|crop_box| crop_box.map(|value| value as f32)).collect();
        let want_boxes: Vec<[f32; 4]> = serde_json::from_value(want["boxes"].clone()).unwrap();
        assert_eq!(boxes, want_boxes, "{name}: crop {i}'s boxes (float32)");
        compare(&format!("crop {i}"), &serde_json::to_value(&crop.row).unwrap(), &want["row"], diff);
    }
}

#[test]
fn the_users_cut_offs_match_python() {
    let Ok(list) = fs::read_dir(parity_root().join("faint")) else {
        eprintln!("no test_out/parity/faint");
        return;
    };
    for entry in list.flatten() {
        let want = read(&entry.path());
        let tracks: Tracks = serde_json::from_value(read(Path::new(want["tracks"].as_str().unwrap()))).unwrap();
        let near = want["near"].as_f64().unwrap();
        let offset = want["offset"].as_f64().unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();

        let scored = faint_scores(&tracks.frames, near);
        let score_row = |track: &TrackScore| json!([track.id, track.score, track.frames]);
        let scores: Vec<Value> = scored.scores.iter().map(score_row).collect();
        assert_eq!(Value::from(scores), want["scores"], "{name}: scores");
        assert_eq!(scored.level, want["level"].as_f64(), "{name}: level");

        let cut = without_faint(&tracks.frames, offset, near);
        assert_eq!(cut.cut, want["cut"].as_f64(), "{name}: cut");
        assert_eq!(cut.gone as u64, want["gone"].as_u64().unwrap(), "{name}: tracks cut");
        let points: usize = cut.frames.iter().map(|frame| frame.t.len()).sum();
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
        let mut diff = Diff::default();
        compare_crops(&name, &crops, want["crops"].as_array().unwrap(), &mut diff);
        let (tracks_scored, gone, crop_count) = (scored.scores.len(), cut.gone, crops.len());
        eprintln!(
            "{name}: {tracks_scored} tracks scored, cut {:?} ({gone} tracks), {crop_count} crops; {} numbers equal \
             within 1e-9, not to the bit",
            cut.cut, diff.close
        );
        diff.assert_none(&name);
    }
}
