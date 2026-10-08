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
use criterion::measurement::WallTime;
use criterion::{BatchSize, BenchmarkGroup, Criterion, SamplingMode};

use crate::{FEW_SAMPLES, FEWEST_SAMPLES, inputs};

/// av1's key frames at 1280 x 720, YUV 4:2:0, as ffmpeg gave them to Python.
const AV1_KEYS: &str = "test_out/parity/av1/keys.yuv";
/// av1's key frame count, which the HUD watch's key pass takes from its two decoded frames in turn.
const AV1_KEY_FRAMES: usize = 25;
/// Two frames (0 and 200) of a recording as decoded, and as ffmpeg's 720p RGB24 (test_out/parity/convert/).
const CONVERT: &str = "test_out/parity/convert";
/// The frame numbers of the decoded frames in test_out/parity/convert/.
const FRAME_NUMBERS: [usize; 2] = [0, 200];
/// The bytes of a 1280 x 720 yuv420p frame: the Y plane, then U and V at a quarter of its size each.
const KEY_FRAME_BYTES: usize = W * H * 3 / 2;
/// flower's stored gray frames the camera watch takes in turn.
const CAMERA_FRAMES: usize = 8;
/// flower's stored gray frame the reading bench reads (with the one before it).
const READING_FRAME: usize = 3;

/// A recording whose decoded frames the benches read.
struct Decoded {
    /// The benches' name for it.
    name: &'static str,
    /// Its frames' key in test_out/parity/convert/.
    key: &'static str,
    /// Its width, in pixels.
    width: usize,
    /// Its height, in pixels.
    height: usize,
    /// Whether its Y spans 0..255.
    full_range: bool,
}

/// av1 at 2560 x 1440, full range: its size takes the converter's 2:1 shortcut.
const AV1_2560: Decoded = Decoded { name: "2560", key: "av1_2560_pc", width: 2560, height: 1440, full_range: true };
/// An h264 recording at 1920 x 1080, limited range: its size goes through swscale's full pipeline.
const H264_1920: Decoded = Decoded { name: "1920", key: "h264_1920_tv", width: 1920, height: 1080, full_range: false };
/// The recordings the convert and HUD benches run on.
const DECODED: [Decoded; 2] = [AV1_2560, H264_1920];

/// A decoded frame of `key` (frame 0 or 200), YUV 4:2:0 at its own size.
fn decoded(bench: &str, key: &str, number: usize) -> Option<Vec<u8>> {
    inputs::bytes(bench, &format!("{CONVERT}/{key}_{number}_src.yuv"))
}

/// Both decoded frames of `key`, or fewer where one is missing.
fn decoded_frames(bench: &str, key: &str) -> Vec<Vec<u8>> {
    FRAME_NUMBERS.into_iter().filter_map(|number| decoded(bench, key, number)).collect()
}

/// The fixed map: one key frame's `contrast`, and one key frame added to the map (av1's 25 in turn).
pub fn fixed(criterion: &mut Criterion) {
    let Some(keys) = inputs::bytes("fixed", AV1_KEYS) else { return };
    let frames: Vec<&[u8]> = keys.chunks_exact(KEY_FRAME_BYTES).collect();
    let mut group = criterion.benchmark_group("fixed");
    group.sample_size(FEW_SAMPLES);
    group.bench_function("contrast", |bencher| bencher.iter(|| contrast(black_box(frames[0]))));
    // one key frame into the map, av1's 25 in turn
    let (mut map, mut next) = (FixedMap::default(), 0);
    group.bench_function("add", |bencher| {
        bencher.iter(|| {
            map.add(black_box(frames[next % frames.len()]));
            next += 1;
        })
    });
    group.finish();
}

/// A decoded frame converted to 720p RGB24, and its Y plane to 720p luma, at each recording's size.
pub fn convert(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("convert");
    group.sample_size(FEW_SAMPLES);
    // 2560 x 1440 takes the 2:1 shortcut; 1920 x 1080 goes through swscale's full pipeline
    for Decoded { name, key, width, height, full_range } in DECODED {
        let Some(frame) = decoded("convert", key, 0) else { continue };
        let mut converter = Converter::new(width, height, Matrix::Bt709, full_range);
        let mut rgb = vec![0u8; DST_W * DST_H * 3];
        group.bench_function(format!("rgb24_{name}"), |bencher| {
            bencher.iter(|| converter.rgb24(black_box(&frame), &mut rgb))
        });
        let mut luma = vec![0u8; DST_W * DST_H];
        let y_plane = &frame[..width * height];
        group.bench_function(format!("luma_{name}"), |bencher| {
            bencher.iter(|| converter.luma(black_box(y_plane), &mut luma))
        });
    }
    group.finish();
}

/// The camera watch's add (a frame against the one before it) and a frame's reading from its tiles' shifts, on
/// flower's frames as the camera reads them (720p luma): pairs of frames in a row, from
/// python/retired/tests/fixtures.py.
fn camera_watch(group: &mut BenchmarkGroup<WallTime>, dir: &str, left_out: &[bool]) {
    let bench = "camera";
    let gray = inputs::bytes(bench, &format!("{dir}/review/gray.raw"));
    let stored: Option<serde_json::Value> = inputs::json(bench, &format!("{dir}/review/gray.json"));
    let tracks: Option<Tracks> = inputs::json(bench, &format!("{dir}/review/tracks.json"));
    let (Some(gray), Some(stored), Some(tracks)) = (gray, stored, tracks) else { return };
    let frames: Vec<&[u8]> = gray.chunks_exact(W * H).take(CAMERA_FRAMES).collect();
    let rgb = vec![0u8; W * H * 3];
    let (mut watch, mut next) = (CameraWatch::new(left_out), 0);
    group.bench_function("add", |bencher| {
        bencher.iter(|| {
            watch.add(black_box(frames[next % frames.len()]), &rgb);
            next += 1;
        })
    });
    // a frame's reading from its tiles' shifts, the tiles with a target in it or the frame before left out
    let mut watch = CameraWatch::new(left_out);
    watch.add(frames[READING_FRAME - 1], &rgb);
    watch.add(frames[READING_FRAME], &rgb);
    let frame_number = stored["frames"][READING_FRAME].as_u64().unwrap() as usize;
    let near = [&tracks.frames[frame_number - 1], &tracks.frames[frame_number]];
    group.bench_function("reading", |bencher| bencher.iter(|| watch.reading(black_box(&watch.shifts[1]), &near)));
}

/// The camera watch: the tiles left out by the areas and the fixed map, a frame's add and reading on flower, and the
/// countdown test on an RGB frame.
pub fn camera(criterion: &mut Criterion) {
    let bench = "camera";
    let dir = "test_out/parity/flower";
    let fixed = inputs::npy(bench, &format!("{dir}/fixed.npy"), W * H);
    let mut group = criterion.benchmark_group("camera");
    group.sample_size(FEW_SAMPLES);
    if let Some(fixed) = &fixed {
        let keep = Mask::without(&overlay_shares());
        group.bench_function("excluded", |bencher| bencher.iter(|| excluded(keep.kept(), black_box(fixed))));
        camera_watch(&mut group, dir, &excluded(keep.kept(), fixed));
    }
    if let Some(rgb) = inputs::bytes(bench, &format!("{CONVERT}/av1_2560_pc_0_rgb24.raw")) {
        group.bench_function("countdown_showing", |bencher| bencher.iter(|| countdown_showing(black_box(&rgb))));
    }
    group.finish();
}

/// What av1's key frames give the HUD watch, from its two decoded frames taken in turn as its 25 key frames.
fn hud_keys(recording: &Decoded, y_planes: &[&[u8]]) -> HudKeys {
    let mut watch = HudWatch::new(recording.width, recording.height, recording.full_range);
    for i in 0..AV1_KEY_FRAMES {
        watch.add_key(y_planes[i % y_planes.len()]);
    }
    watch.keys()
}

/// The HUD watch: the key frames' pass at 2560 x 1440, and a frame's add at each recording's size.
pub fn hud(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("hud");
    group.sample_size(FEW_SAMPLES);
    for recording in &DECODED {
        let (name, key) = (recording.name, recording.key);
        let frames = decoded_frames("hud", key);
        if frames.len() < FRAME_NUMBERS.len() {
            continue;
        }
        let y_planes: Vec<&[u8]> = frames.iter().map(|frame| &frame[..recording.width * recording.height]).collect();
        let keys = hud_keys(recording, &y_planes);
        if keys.session().is_none() {
            eprintln!("hud: no KovaaK's box in {key}'s frames: add_{name} reads only Aim Lab's band");
        }
        if name == "2560" {
            // the key frames' pass: each key frame's box region, then the box worked out from their median
            group.bench_function("keys_2560", |bencher| bencher.iter(|| hud_keys(recording, black_box(&y_planes))));
        }
        let mut watch = HudWatch::from_keys(recording.width, recording.height, recording.full_range, &keys);
        group.bench_function(format!("add_{name}"), |bencher| bencher.iter(|| watch.add(black_box(y_planes[0]))));
    }
    group.finish();
}

/// The pop-up watch's look at av1's excluded areas in one frame.
pub fn popup(criterion: &mut Criterion) {
    let meta: Option<serde_json::Value> = inputs::json("popup", "test_out/parity/av1/meta.json");
    let rgb = inputs::bytes("popup", &format!("{CONVERT}/av1_2560_pc_0_rgb24.raw"));
    let (Some(meta), Some(rgb)) = (meta, rgb) else { return };
    let areas = inputs::areas(&meta);
    let mut group = criterion.benchmark_group("popup");
    // a frame the watch looks at (every popup::STEP-th frame): av1's excluded areas, each one's pixels and 3 x 3 means
    group.bench_function("add_look", |bencher| {
        bencher.iter_batched_ref(|| AreaWatch::new(&areas), |watch| watch.add(black_box(&rgb)), BatchSize::SmallInput)
    });
    group.finish();
}

/// The area finder's `finish` on av1's key frames, with KovaaK's box as the HUD watch finds it.
pub fn areas(criterion: &mut Criterion) {
    let Some(keys) = inputs::bytes("areas", AV1_KEYS) else { return };
    let mut finder = AreaFinder::new();
    for frame in keys.chunks_exact(KEY_FRAME_BYTES) {
        finder.add(frame);
    }
    // KovaaK's box as the HUD watch finds it in av1's decoded frames, as the native review gives it
    let frames = decoded_frames("areas", AV1_2560.key);
    let y_planes: Vec<&[u8]> = frames.iter().map(|frame| &frame[..AV1_2560.width * AV1_2560.height]).collect();
    let session = (y_planes.len() == FRAME_NUMBERS.len()).then(|| hud_keys(&AV1_2560, &y_planes).session()).flatten();
    let mut group = criterion.benchmark_group("areas");
    group.sample_size(FEWEST_SAMPLES).sampling_mode(SamplingMode::Flat);
    group.bench_function("finish_av1", |bencher| {
        bencher.iter_batched(|| finder.clone(), |cloned| cloned.finish(black_box(session)), BatchSize::LargeInput)
    });
    group.finish();
}
