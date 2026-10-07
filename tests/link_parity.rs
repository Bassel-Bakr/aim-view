//! `track::link` against Python's `link` on real recordings: the fixtures python/retired/tests/fixtures.py writes to
//! test_out/parity/<name>/ (dets.json: link's input, frames.json: its output). Every value must be equal, to the bit.

use std::fs;

use aimview::track::{ModelBox, Spot, TrackFrame, link};
use serde_json::Value;

mod common;
use common::fixture_dirs;

/// How many differing frames a fixture prints before it fails.
const SHOWN_FRAMES: usize = 3;

/// A row of Python's detections: [x, y, area] or [x, y, area, w, h, score].
fn spot(row: &Value) -> Spot {
    let values: Vec<f64> = row.as_array().unwrap().iter().map(|number| number.as_f64().unwrap()).collect();
    Spot {
        x: values[0],
        y: values[1],
        area: values[2] as i64,
        model: (values.len() > 3).then(|| ModelBox { w: values[3], h: values[4], score: values[5] }),
    }
}

/// Every fixture's linked frames equal Python's, frame for frame.
#[test]
fn link_matches_python() {
    let dirs = fixture_dirs(&["dets.json", "frames.json"]);
    if dirs.is_empty() {
        eprintln!("no fixtures in test_out/parity: they are frozen (python/retired/tests/fixtures.py made them)");
        return;
    }
    for dir in dirs {
        let dets: Vec<Vec<Value>> = serde_json::from_str(&fs::read_to_string(dir.join("dets.json")).unwrap()).unwrap();
        let want: Vec<TrackFrame> =
            serde_json::from_str(&fs::read_to_string(dir.join("frames.json")).unwrap()).unwrap();
        let frames: Vec<Vec<Spot>> = dets.iter().map(|frame| frame.iter().map(spot).collect()).collect();
        let got = link(&frames);
        assert_eq!(got.len(), want.len(), "{}: frame count", dir.display());
        let wrong: Vec<usize> = (0..got.len()).filter(|&i| got[i] != want[i]).collect();
        for &i in wrong.iter().take(SHOWN_FRAMES) {
            eprintln!("{} frame {i}:\n  rust   {:?}\n  python {:?}", dir.display(), got[i], want[i]);
        }
        assert!(wrong.is_empty(), "{}: {} of {} frames differ", dir.display(), wrong.len(), got.len());
        eprintln!("{}: {} frames equal", dir.display(), got.len());
    }
}
