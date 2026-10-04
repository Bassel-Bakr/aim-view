//! The track step after the detector: each frame's boxes kept or dropped (`keep`), then the frames linked into tracks
//! (`link`, with each frame's view shift).

use std::hint::black_box;

use aimview::track::{Mask, ModelBox, RawBox, Spot, Tracks, keep, link};
use criterion::{Criterion, SamplingMode};
use serde_json::Value;

use crate::inputs;

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

/// The targets of each frame of a run's tracks, as `link` took them (places to 4 decimals, boxes to 3).
fn spots(tracks: &Tracks) -> Vec<Vec<Spot>> {
    tracks
        .frames
        .iter()
        .map(|f| {
            let boxes = f.wh.as_ref().zip(f.s.as_ref()).filter(|(wh, _)| wh.len() == f.t.len());
            (0..f.t.len())
                .map(|j| Spot {
                    x: f.t[j].1,
                    y: f.t[j].2,
                    area: f.a[j],
                    model: boxes.map(|(wh, s)| ModelBox { w: wh[j].0, h: wh[j].1, score: s[j] }),
                })
                .collect()
        })
        .collect()
}

pub fn track(c: &mut Criterion) {
    let mut g = c.benchmark_group("track");
    g.sample_size(10).sampling_mode(SamplingMode::Flat);
    // av1's every frame: the detector's boxes, the recording's excluded areas and target count (Python's fixture)
    let meta: Option<Value> = inputs::json("track/keep_av1", "test_out/parity/av1/meta.json");
    let raw: Option<Rows> = inputs::json("track/keep_av1", "test_out/parity/av1/raw.json");
    if let (Some(meta), Some(raw)) = (meta, raw) {
        let mask = Mask::without(&inputs::areas(&meta));
        let cap = meta["cap"].as_u64().map(|c| c as usize);
        let boxes: Vec<Vec<RawBox>> = raw
            .iter()
            .map(|f| {
                f.iter()
                    .map(|v| RawBox { cx: v[0] as f32, cy: v[1] as f32, w: v[2] as f32, h: v[3] as f32, score: v[4] as f32 })
                    .collect()
            })
            .collect();
        g.bench_function("keep_av1", |b| {
            b.iter(|| boxes.iter().map(|f| keep(black_box(f), &mask, cap).len()).sum::<usize>())
        });
    }
    // av1's targets as keep gave them to Python's link ([x, y, area, w, h, score] a target)
    if let Some(dets) = inputs::json::<Rows>("track/link_av1", "test_out/parity/av1/dets.json") {
        let frames: Vec<Vec<Spot>> = dets
            .iter()
            .map(|f| {
                f.iter()
                    .map(|v| Spot {
                        x: v[0],
                        y: v[1],
                        area: v[2] as i64,
                        model: (v.len() > 3).then(|| ModelBox { w: v[3], h: v[4], score: v[5] }),
                    })
                    .collect()
            })
            .collect();
        g.bench_function("link_av1", |b| b.iter(|| link(black_box(&frames))));
    }
    for (name, path) in CLUTTERED {
        if let Some(tracks) = inputs::json::<Tracks>(&format!("track/{name}"), path) {
            let frames = spots(&tracks);
            g.bench_function(name, |b| b.iter(|| link(black_box(&frames))));
        }
    }
    g.finish();
}
