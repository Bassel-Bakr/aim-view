//! The track step after the detector: each frame's boxes kept or dropped (`keep`), then the frames linked into tracks
//! (`link`, with each frame's view shift).

use std::hint::black_box;

use aimview::track::{Mask, ModelBox, RawBox, Spot, Tracks, keep, link};
use criterion::{Criterion, SamplingMode};
use serde_json::Value;

use crate::{FEWEST_SAMPLES, inputs};

/// Each frame's rows of numbers, as the fixtures keep boxes and targets.
type Rows = Vec<Vec<Vec<f64>>>;

/// Two cluttered runs' tracks (many targets a frame), where `link`'s view shift costs the most.
const CLUTTERED: [(&str, &str); 2] = [
    ("link_2007_1w6ts_aimlab", "test_out/vod_app/2007_1w6ts_aimlab___1_WR/tracks.json"),
    (
        "link_10_sphere_hipfire",
        "test_out/vod_model/accept/full_v3/vods/10 Sphere Hipfire Extra Small - 1550 - 2026.08.26-04.40.26/tracks.json",
    ),
];

/// A detector's box from a fixture's row: [cx, cy, w, h, score], float32 values.
fn raw_box(row: &[f64]) -> RawBox {
    RawBox { cx: row[0] as f32, cy: row[1] as f32, w: row[2] as f32, h: row[3] as f32, score: row[4] as f32 }
}

/// A target from a row of the targets keep gave Python's link: [x, y, area] or [x, y, area, w, h, score].
fn spot(row: &[f64]) -> Spot {
    Spot {
        x: row[0],
        y: row[1],
        area: row[2] as i64,
        model: (row.len() > 3).then(|| ModelBox { w: row[3], h: row[4], score: row[5] }),
    }
}

/// The targets of each frame of a run's tracks, as `link` took them (places to 4 decimals, boxes to 3).
fn spots(tracks: &Tracks) -> Vec<Vec<Spot>> {
    tracks
        .frames
        .iter()
        .map(|frame| {
            let boxes = frame.wh.as_ref().zip(frame.s.as_ref()).filter(|(sizes, _)| sizes.len() == frame.t.len());
            (0..frame.t.len())
                .map(|j| Spot {
                    x: frame.t[j].1,
                    y: frame.t[j].2,
                    area: frame.a[j],
                    model: boxes.map(|(sizes, scores)| ModelBox { w: sizes[j].0, h: sizes[j].1, score: scores[j] }),
                })
                .collect()
        })
        .collect()
}

pub fn track(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("track");
    group.sample_size(FEWEST_SAMPLES).sampling_mode(SamplingMode::Flat);
    // av1's every frame: the detector's boxes, the recording's excluded areas and target count (Python's fixture)
    let meta: Option<Value> = inputs::json("track/keep_av1", "test_out/parity/av1/meta.json");
    let raw: Option<Rows> = inputs::json("track/keep_av1", "test_out/parity/av1/raw.json");
    if let (Some(meta), Some(raw)) = (meta, raw) {
        let mask = Mask::without(&inputs::areas(&meta));
        let target_count = meta["cap"].as_u64().map(|count| count as usize);
        let boxes: Vec<Vec<RawBox>> = raw.iter().map(|frame| frame.iter().map(|row| raw_box(row)).collect()).collect();
        group.bench_function("keep_av1", |bencher| {
            bencher.iter(|| boxes.iter().map(|frame| keep(black_box(frame), &mask, target_count).len()).sum::<usize>())
        });
    }
    // av1's targets as keep gave them to Python's link ([x, y, area, w, h, score] a target)
    if let Some(dets) = inputs::json::<Rows>("track/link_av1", "test_out/parity/av1/dets.json") {
        let frames: Vec<Vec<Spot>> = dets.iter().map(|frame| frame.iter().map(|row| spot(row)).collect()).collect();
        group.bench_function("link_av1", |bencher| bencher.iter(|| link(black_box(&frames))));
    }
    for (name, path) in CLUTTERED {
        if let Some(tracks) = inputs::json::<Tracks>(&format!("track/{name}"), path) {
            let frames = spots(&tracks);
            group.bench_function(name, |bencher| bencher.iter(|| link(black_box(&frames))));
        }
    }
    group.finish();
}
