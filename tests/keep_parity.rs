//! `track::keep` against Python's `keep` (python/retired/review.py, `track_model`) on real recordings: the detector's
//! raw boxes per frame (raw.json) must give the same targets (dets.json), to the bit, with the recording's excluded
//! areas and target count (meta.json), and the frames where a pop-up area is off kept again (`reopen`, with the pop-ups
//! Python found, meta.json's `showing`). Fixtures from python/retired/tests/fixtures.py, in test_out/parity/<name>/.

use std::path::Path;

use aimview::track::{Mask, ModelBox, RawBox, Spot, keep, reopen};
use serde_json::Value;

mod common;
use common::{excluded_areas, fixture_dirs, read, showing_frames};

/// How many differing frames a fixture prints before it fails.
const SHOWN_FRAMES: usize = 3;

/// A JSON array of numbers as floats.
fn numbers(row: &Value) -> Vec<f64> {
    row.as_array().unwrap().iter().map(|number| number.as_f64().unwrap()).collect()
}

/// A frame's raw boxes from the detector (a frame of raw.json): rows of [cx, cy, w, h, score], in frame pixels.
fn raw_boxes(frame: &Value) -> Vec<RawBox> {
    let raw_box = |row: &Value| {
        let values: Vec<f32> = numbers(row).iter().map(|&value| value as f32).collect();
        RawBox { cx: values[0], cy: values[1], w: values[2], h: values[3], score: values[4] }
    };
    frame.as_array().unwrap().iter().map(raw_box).collect()
}

/// A frame's targets as Python kept them (a frame of dets.json): rows of [x, y, area, w, h, score].
fn kept_spots(frame: &Value) -> Vec<Spot> {
    let spot = |row: &Value| {
        let values = numbers(row);
        let model = Some(ModelBox { w: values[3], h: values[4], score: values[5] });
        Spot { x: values[0], y: values[1], area: values[2] as i64, model }
    };
    frame.as_array().unwrap().iter().map(spot).collect()
}

/// Keeps a fixture's every frame's targets as the core does, pop-ups reopened, and fails when any frame differs from
/// Python's, printing the first few.
fn check_fixture(dir: &Path) {
    let meta = read(&dir.join("meta.json"));
    let areas = excluded_areas(&meta);
    let mask = Mask::without(&areas);
    let target_count = meta["cap"].as_u64().map(|count| count as usize);
    let raw = read(&dir.join("raw.json"));
    let want = read(&dir.join("dets.json"));
    let (raw, want) = (raw.as_array().unwrap(), want.as_array().unwrap());
    assert_eq!(raw.len(), want.len());
    let showing: Vec<Option<Vec<bool>>> = meta["showing"].as_array().unwrap().iter().map(showing_frames).collect();
    let raw_frames: Vec<Vec<RawBox>> = raw.iter().map(raw_boxes).collect();
    let mut got: Vec<Vec<Spot>> = raw_frames.iter().map(|boxes| keep(boxes, &mask, target_count)).collect();
    reopen(&raw_frames, &mut got, &areas, &showing, target_count);
    let mut wrong = 0;
    for (i, (spots, want_frame)) in got.iter().zip(want).enumerate() {
        let expected = kept_spots(want_frame);
        if *spots != expected {
            if wrong < SHOWN_FRAMES {
                eprintln!("{} frame {i}:\n  rust   {spots:?}\n  python {expected:?}", dir.display());
            }
            wrong += 1;
        }
    }
    assert_eq!(wrong, 0, "{}: {wrong} of {} frames differ", dir.display(), raw.len());
    eprintln!("{}: {} frames equal", dir.display(), raw.len());
}

/// Every fixture's kept targets equal Python's, frame for frame.
#[test]
fn keep_matches_python() {
    let dirs = fixture_dirs(&["raw.json"]);
    if dirs.is_empty() {
        eprintln!("no fixtures in test_out/parity: they are frozen (python/retired/tests/fixtures.py made them)");
        return;
    }
    for dir in dirs {
        check_fixture(&dir);
    }
}
