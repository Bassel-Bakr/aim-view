//! The stages a frame goes through: the key frames' fixed map and area finder, the conversion to 720p, the camera
//! watch, the HUD watch and the pop-up areas' watch.

use std::hint::black_box;

use aimview::areas::AreaFinder;
use aimview::camera::{CameraWatch, countdown_showing, excluded};
use aimview::convert::{Converter, DST_H, DST_W, Matrix};
use aimview::fixed::{FixedMap, contrast};
use aimview::geometry::{H, W, overlay_shares};
use aimview::hud::{HudKeys, HudWatch};
use aimview::popup::AreaWatch;
use aimview::track::{Mask, Tracks};
use criterion::{BatchSize, Criterion, SamplingMode};

use crate::inputs;

/// av1's key frames at 1280 x 720, YUV 4:2:0, as ffmpeg gave them to Python.
const AV1_KEYS: &str = "test_out/parity/av1/keys.yuv";
/// Two frames (0 and 200) of a recording as decoded, and as ffmpeg's 720p RGB24 (test_out/parity/convert/).
const CONVERT: &str = "test_out/parity/convert";

/// The decoded frames the benches read: a name, the frames' key in test_out/parity/convert/, size and range.
const DECODED: [(&str, &str, usize, usize, bool); 2] =
    [("2560", "av1_2560_pc", 2560, 1440, true), ("1920", "h264_1920_tv", 1920, 1080, false)];

/// A decoded frame of `key` (0 or 200), YUV 4:2:0 at its own size.
fn decoded(bench: &str, key: &str, n: usize) -> Option<Vec<u8>> {
    inputs::bytes(bench, &format!("{CONVERT}/{key}_{n}_src.yuv"))
}

pub fn fixed(c: &mut Criterion) {
    let Some(keys) = inputs::bytes("fixed", AV1_KEYS) else { return };
    let frames: Vec<&[u8]> = keys.chunks_exact(W * H * 3 / 2).collect();
    let mut g = c.benchmark_group("fixed");
    g.sample_size(20);
    g.bench_function("contrast", |b| b.iter(|| contrast(black_box(frames[0]))));
    // one key frame into the map, av1's 25 in turn
    let (mut map, mut k) = (FixedMap::default(), 0);
    g.bench_function("add", |b| {
        b.iter(|| {
            map.add(black_box(frames[k % frames.len()]));
            k += 1;
        })
    });
    g.finish();
}

pub fn convert(c: &mut Criterion) {
    let mut g = c.benchmark_group("convert");
    g.sample_size(20);
    // 2560 x 1440 takes the 2:1 shortcut; 1920 x 1080 goes through swscale's full pipeline
    for (name, key, w, h, full) in DECODED {
        let Some(src) = decoded("convert", key, 0) else { continue };
        let mut conv = Converter::new(w, h, Matrix::Bt709, full);
        let mut rgb = vec![0u8; DST_W * DST_H * 3];
        g.bench_function(format!("rgb24_{name}"), |b| b.iter(|| conv.rgb24(black_box(&src), &mut rgb)));
        let mut luma = vec![0u8; DST_W * DST_H];
        g.bench_function(format!("luma_{name}"), |b| b.iter(|| conv.luma(black_box(&src[..w * h]), &mut luma)));
    }
    g.finish();
}

pub fn camera(c: &mut Criterion) {
    let bench = "camera";
    let dir = "test_out/parity/flower";
    let fixed = inputs::npy(bench, &format!("{dir}/fixed.npy"), W * H);
    let mut g = c.benchmark_group("camera");
    g.sample_size(20);
    if let Some(fixed) = &fixed {
        let keep = Mask::without(&overlay_shares());
        g.bench_function("excluded", |b| b.iter(|| excluded(keep.kept(), black_box(fixed))));
        // flower's frames as the camera reads them (720p luma): pairs of frames in a row, from tests/fixtures.py
        let gray = inputs::bytes(bench, &format!("{dir}/review/gray.raw"));
        let stored: Option<serde_json::Value> = inputs::json(bench, &format!("{dir}/review/gray.json"));
        let tracks: Option<Tracks> = inputs::json(bench, &format!("{dir}/review/tracks.json"));
        if let (Some(gray), Some(stored), Some(tracks)) = (gray, stored, tracks) {
            let bad = excluded(keep.kept(), fixed);
            let frames: Vec<&[u8]> = gray.chunks_exact(W * H).take(8).collect();
            let rgb = vec![0u8; W * H * 3];
            let (mut watch, mut k) = (CameraWatch::new(&bad), 0);
            g.bench_function("add", |b| {
                b.iter(|| {
                    watch.add(black_box(frames[k % frames.len()]), &rgb);
                    k += 1;
                })
            });
            // a frame's reading from its tiles' shifts, the tiles with a target in it or the frame before left out
            let mut watch = CameraWatch::new(&bad);
            watch.add(frames[2], &rgb);
            watch.add(frames[3], &rgb);
            let i = stored["frames"][3].as_u64().unwrap() as usize;
            let near = [&tracks.frames[i - 1], &tracks.frames[i]];
            g.bench_function("reading", |b| b.iter(|| watch.reading(black_box(&watch.shifts[1]), &near)));
        }
    }
    if let Some(rgb) = inputs::bytes(bench, &format!("{CONVERT}/av1_2560_pc_0_rgb24.raw")) {
        g.bench_function("countdown_showing", |b| b.iter(|| countdown_showing(black_box(&rgb))));
    }
    g.finish();
}

/// What av1's key frames give the HUD watch, from its two decoded frames taken in turn as its 25 key frames.
fn hud_keys(w: usize, h: usize, full: bool, ys: &[&[u8]]) -> HudKeys {
    let mut watch = HudWatch::new(w, h, full);
    for k in 0..25 {
        watch.add_key(ys[k % ys.len()]);
    }
    watch.keys()
}

pub fn hud(c: &mut Criterion) {
    let mut g = c.benchmark_group("hud");
    g.sample_size(20);
    for (name, key, w, h, full) in DECODED {
        let frames: Vec<Vec<u8>> = [0, 200].into_iter().filter_map(|n| decoded("hud", key, n)).collect();
        if frames.len() < 2 {
            continue;
        }
        let ys: Vec<&[u8]> = frames.iter().map(|f| &f[..w * h]).collect();
        let keys = hud_keys(w, h, full, &ys);
        if keys.session().is_none() {
            eprintln!("hud: no KovaaK's box in {key}'s frames: add_{name} reads only Aim Lab's band");
        }
        if name == "2560" {
            // the key frames' pass: each key frame's box region, then the box worked out from their median
            g.bench_function("keys_2560", |b| b.iter(|| hud_keys(w, h, full, black_box(&ys))));
        }
        let mut watch = HudWatch::from_keys(w, h, full, &keys);
        g.bench_function(format!("add_{name}"), |b| b.iter(|| watch.add(black_box(ys[0]))));
    }
    g.finish();
}

pub fn popup(c: &mut Criterion) {
    let meta: Option<serde_json::Value> = inputs::json("popup", "test_out/parity/av1/meta.json");
    let rgb = inputs::bytes("popup", &format!("{CONVERT}/av1_2560_pc_0_rgb24.raw"));
    let (Some(meta), Some(rgb)) = (meta, rgb) else { return };
    let areas = inputs::areas(&meta);
    let mut g = c.benchmark_group("popup");
    // a frame the watch looks at (every popup::STEP-th frame): av1's excluded areas, each one's pixels and 3 x 3 means
    g.bench_function("add_look", |b| {
        b.iter_batched_ref(|| AreaWatch::new(&areas), |w| w.add(black_box(&rgb)), BatchSize::SmallInput)
    });
    g.finish();
}

pub fn areas(c: &mut Criterion) {
    let Some(keys) = inputs::bytes("areas", AV1_KEYS) else { return };
    let mut finder = AreaFinder::new();
    for f in keys.chunks_exact(W * H * 3 / 2) {
        finder.add(f);
    }
    // KovaaK's box as the HUD watch finds it in av1's decoded frames, as the native review gives it
    let frames: Vec<Vec<u8>> = [0, 200].into_iter().filter_map(|n| decoded("areas", "av1_2560_pc", n)).collect();
    let ys: Vec<&[u8]> = frames.iter().map(|f| &f[..2560 * 1440]).collect();
    let session = (ys.len() == 2).then(|| hud_keys(2560, 1440, true, &ys).session()).flatten();
    let mut g = c.benchmark_group("areas");
    g.sample_size(10).sampling_mode(SamplingMode::Flat);
    g.bench_function("finish_av1", |b| {
        b.iter_batched(|| finder.clone(), |f| f.finish(black_box(session)), BatchSize::LargeInput)
    });
    g.finish();
}
