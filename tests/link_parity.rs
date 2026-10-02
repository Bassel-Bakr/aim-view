//! `track::link` against Python's `link` on real recordings: the fixtures tests/fixtures.py writes to
//! test_out/parity/<name>/ (dets.json: link's input, frames.json: its output). Every value must be equal, to the bit.

use std::fs;
use std::path::PathBuf;

use aimview::track::{ModelBox, Spot, TrackFrame, link};
use serde_json::Value;

fn fixtures() -> Vec<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_out/parity");
    let mut dirs: Vec<PathBuf> = fs::read_dir(root)
        .map(|d| d.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default();
    dirs.retain(|d| d.join("dets.json").exists() && d.join("frames.json").exists());
    dirs.sort();
    dirs
}

/// A row of Python's detections: [x, y, area] or [x, y, area, w, h, score].
fn spot(row: &Value) -> Spot {
    let v: Vec<f64> = row.as_array().unwrap().iter().map(|n| n.as_f64().unwrap()).collect();
    Spot {
        x: v[0],
        y: v[1],
        area: v[2] as i64,
        model: (v.len() > 3).then(|| ModelBox { w: v[3], h: v[4], score: v[5] }),
    }
}

#[test]
fn link_matches_python() {
    let dirs = fixtures();
    if dirs.is_empty() {
        eprintln!("no fixtures in test_out/parity: run python tests/fixtures.py <video>");
        return;
    }
    for dir in dirs {
        let dets: Vec<Vec<Value>> = serde_json::from_str(&fs::read_to_string(dir.join("dets.json")).unwrap()).unwrap();
        let want: Vec<TrackFrame> =
            serde_json::from_str(&fs::read_to_string(dir.join("frames.json")).unwrap()).unwrap();
        let frames: Vec<Vec<Spot>> = dets.iter().map(|f| f.iter().map(spot).collect()).collect();
        let got = link(&frames);
        assert_eq!(got.len(), want.len(), "{}: frame count", dir.display());
        let wrong: Vec<usize> = (0..got.len()).filter(|&i| got[i] != want[i]).collect();
        for &i in wrong.iter().take(3) {
            eprintln!("{} frame {i}:\n  rust   {:?}\n  python {:?}", dir.display(), got[i], want[i]);
        }
        assert!(wrong.is_empty(), "{}: {} of {} frames differ", dir.display(), wrong.len(), got.len());
        eprintln!("{}: {} frames equal", dir.display(), got.len());
    }
}
