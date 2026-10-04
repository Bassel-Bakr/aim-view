//! `track::keep` against Python's `keep` (python/retired/review.py, `track_model`) on real recordings: the detector's
//! raw boxes per frame (raw.json) must give the same targets (dets.json), to the bit, with the recording's excluded
//! areas and target count (meta.json), and the frames where a pop-up area is off kept again (`reopen`, with the pop-ups
//! Python found, meta.json's `showing`). Fixtures from python/retired/tests/fixtures.py, in test_out/parity/<name>/.

use std::fs;
use std::path::{Path, PathBuf};

use aimview::track::{Mask, ModelBox, RawBox, Spot, keep, reopen};
use serde_json::Value;

fn read(dir: &Path, name: &str) -> Value {
    serde_json::from_str(&fs::read_to_string(dir.join(name)).unwrap()).unwrap()
}

fn numbers(v: &Value) -> Vec<f64> {
    v.as_array().unwrap().iter().map(|n| n.as_f64().unwrap()).collect()
}

#[test]
fn keep_matches_python() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_out/parity");
    let dirs: Vec<PathBuf> = fs::read_dir(root)
        .map(|d| d.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default();
    let dirs: Vec<PathBuf> = dirs.into_iter().filter(|d| d.join("raw.json").exists()).collect();
    if dirs.is_empty() {
        eprintln!("no fixtures in test_out/parity: they are frozen (python/retired/tests/fixtures.py made them)");
        return;
    }
    for dir in dirs {
        let meta = read(&dir, "meta.json");
        let areas: Vec<[f64; 4]> = meta["areas"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| {
                let v: Vec<f64> = a.as_array().unwrap()[..4].iter().map(|n| n.as_f64().unwrap()).collect();
                [v[0], v[1], v[2], v[3]]
            })
            .collect();
        let mask = Mask::without(&areas);
        let cap = meta["cap"].as_u64().map(|c| c as usize);
        let raw = read(&dir, "raw.json");
        let want = read(&dir, "dets.json");
        let (raw, want) = (raw.as_array().unwrap(), want.as_array().unwrap());
        assert_eq!(raw.len(), want.len());
        let shows: Vec<Option<Vec<bool>>> = meta["showing"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_array().map(|f| f.iter().map(|v| v.as_i64() == Some(1)).collect()))
            .collect();
        let raws: Vec<Vec<RawBox>> = raw
            .iter()
            .map(|r| {
                r.as_array()
                    .unwrap()
                    .iter()
                    .map(|b| {
                        let v = numbers(b);
                        RawBox { cx: v[0] as f32, cy: v[1] as f32, w: v[2] as f32, h: v[3] as f32, score: v[4] as f32 }
                    })
                    .collect()
            })
            .collect();
        let mut got: Vec<Vec<Spot>> = raws.iter().map(|b| keep(b, &mask, cap)).collect();
        reopen(&raws, &mut got, &areas, &shows, cap);
        let mut wrong = 0;
        for (i, (g, w)) in got.iter().zip(want).enumerate() {
            let expected: Vec<Spot> = w
                .as_array()
                .unwrap()
                .iter()
                .map(|s| {
                    let v = numbers(s);
                    Spot { x: v[0], y: v[1], area: v[2] as i64, model: Some(ModelBox { w: v[3], h: v[4], score: v[5] }) }
                })
                .collect();
            if *g != expected {
                if wrong < 3 {
                    eprintln!("{} frame {i}:
  rust   {g:?}
  python {expected:?}", dir.display());
                }
                wrong += 1;
            }
        }
        assert_eq!(wrong, 0, "{}: {wrong} of {} frames differ", dir.display(), raw.len());
        eprintln!("{}: {} frames equal", dir.display(), raw.len());
    }
}
