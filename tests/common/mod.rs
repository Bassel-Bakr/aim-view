//! What the parity tests share: the frozen fixtures in test_out/parity (python/retired/tests/fixtures.py made them),
//! reading their JSON, excluded areas, gray frames and tracking inputs, and comparing the core's JSON with Python's:
//! everything that is not a number equal, numbers within a relative tolerance.
#![allow(dead_code)] // each test uses only some of these

use std::fs;
use std::path::{Path, PathBuf};

use aimview::camera::excluded;
use aimview::faint::FaintSetting;
use aimview::geometry::{H, W, overlay_shares};
use aimview::review::{KillTimes, TrackScenario, VideoReadings, review_tracking};
use aimview::track::{Mask, Tracks};
use aimview::tracking::CameraReading;
use serde_json::Value;

/// How far two numbers may differ, relative to the larger, and still count as equal: after the track step the core
/// uses plain floating point, so it copies Python's logic, not its last bits.
const RELATIVE_TOLERANCE: f64 = 1e-9;
/// The smallest size the tolerance is taken of, so that numbers near zero are not held to nothing.
const TOLERANCE_FLOOR: f64 = 1e-3;
/// How many differences a failing comparison prints.
const SHOWN_DIFFERENCES: usize = 30;
/// The pixels of one 1280 x 720 frame's luma.
pub const FRAME_PIXELS: usize = W * H;
/// The least countdown-teal pixels in a frame for KovaaK's countdown bar to count as showing (teal.json).
const COUNTDOWN_TEAL_PIXELS: u32 = 40;

/// Where two JSON values differ (paths), and how many numbers were equal only within the tolerance.
#[derive(Default)]
pub struct Diff {
    pub wrong: Vec<String>,
    pub close: usize,
}

impl Diff {
    /// Prints the first `count` differences, indented.
    pub fn print_wrong(&self, count: usize) {
        for difference in self.wrong.iter().take(count) {
            eprintln!("  {difference}");
        }
    }

    /// Prints the first differences and fails when there is any.
    pub fn assert_none(&self, label: &str) {
        self.print_wrong(SHOWN_DIFFERENCES);
        assert!(self.wrong.is_empty(), "{label}: {} differences", self.wrong.len());
    }
}

pub fn compare(path: &str, got: &Value, want: &Value, diff: &mut Diff) {
    match (got, want) {
        (Value::Number(a), Value::Number(b)) => {
            let (a, b) = (a.as_f64().unwrap(), b.as_f64().unwrap());
            if a != b {
                if (a - b).abs() <= RELATIVE_TOLERANCE * a.abs().max(b.abs()).max(TOLERANCE_FLOOR) {
                    diff.close += 1;
                } else {
                    diff.wrong.push(format!("{path}: {a} against {b}"));
                }
            }
        }
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => {
            for (i, (x, y)) in a.iter().zip(b).enumerate() {
                compare(&format!("{path}[{i}]"), x, y, diff);
            }
        }
        // only the fields Python's output has: a field the core adds (Python has no such thing) is not compared, so a
        // new field needs no change here; a field Python has and the core lacks is a difference
        (Value::Object(a), Value::Object(b)) => {
            for (key, wanted) in b {
                match a.get(key) {
                    Some(field) => compare(&format!("{path}.{key}"), field, wanted, diff),
                    None => diff.wrong.push(format!("{path}.{key}: missing against {wanted}")),
                }
            }
        }
        _ if got == want => {}
        _ => diff.wrong.push(format!("{path}: {got} against {want}")),
    }
}

/// Gives a key in JSON written with older names (Python's) the core's name, so the two compare. `path` leads to the
/// key, with `[]` for every element of an array: `flicks[].n`, or `[].n` in a list. A path that leads nowhere changes
/// nothing.
pub fn rename_key(value: &mut Value, path: &str, name: &str) {
    let Some((step, rest)) = path.split_once('.') else {
        if let Some(found) = value.as_object_mut().and_then(|object| object.remove(path)) {
            value[name] = found;
        }
        return;
    };
    let (key, every) = step.strip_suffix("[]").map_or((step, false), |key| (key, true));
    let child = if key.is_empty() { Some(value) } else { value.get_mut(key) };
    match child {
        Some(Value::Array(elements)) if every => {
            elements.iter_mut().for_each(|element| rename_key(element, rest, name));
        }
        Some(child) if !every => rename_key(child, rest, name),
        _ => {}
    }
}

pub fn read(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

/// A file's text, with bytes that are not UTF-8 replaced (KovaaK's stats files, scenario files).
pub fn read_lossy(path: &Path) -> String {
    String::from_utf8_lossy(&fs::read(path).unwrap()).into_owned()
}

/// test_out/parity, the frozen fixtures.
pub fn parity_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_out/parity")
}

/// The fixtures' folders in test_out/parity that hold every one of `files`, in name order.
pub fn fixture_dirs(files: &[&str]) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(parity_root())
        .map(|entries| entries.filter_map(|entry| entry.ok().map(|entry| entry.path())).collect())
        .unwrap_or_default();
    dirs.retain(|dir| files.iter().all(|file| dir.join(file).exists()));
    dirs.sort();
    dirs
}

/// A fixture's excluded areas (meta.json's `areas`): each one's first four numbers, its box as shares of the frame
/// [x0, y0, x1, y1].
pub fn excluded_areas(meta: &Value) -> Vec<[f64; 4]> {
    let area_box = |area: &Value| {
        let corners = &area.as_array().unwrap()[..4];
        std::array::from_fn(|i| corners[i].as_f64().unwrap())
    };
    meta["areas"].as_array().unwrap().iter().map(area_box).collect()
}

/// Python's decision for one area per frame (an element of meta.json's `showing`): whether the pop-up showed (1), or
/// None where the area is not a pop-up.
pub fn showing_frames(area: &Value) -> Option<Vec<bool>> {
    area.as_array().map(|frames| frames.iter().map(|showing| showing.as_i64() == Some(1)).collect())
}

/// The fixed map in a fixed.npy: NumPy's header, then one byte per pixel of a 1280 x 720 frame.
pub fn fixed_map(npy: &[u8]) -> &[u8] {
    &npy[npy.len() - FRAME_PIXELS..]
}

/// The scenario a recording's name gives ("<scenario> - <score> - <time>"), in lower case: scenarios.json's key.
pub fn scenario_key(video: &str) -> String {
    let name = Path::new(video).file_stem().unwrap().to_string_lossy();
    name.rsplitn(3, " - ").last().unwrap().to_lowercase()
}

/// A camera case's gray frames as python/retired/tests/fixtures.py --review kept them (review/gray.raw: one 1280 x 720
/// luma frame after another, the frame numbers in review/gray.json), and the pixels the camera watch leaves out (the
/// KovOBS overlay and the fixed map's pixels, from fixed.npy).
pub struct GrayFrames {
    pub listing: Value,
    pub numbers: Vec<usize>,
    pub luma: Vec<u8>,
    pub left_out: Vec<bool>,
}

impl GrayFrames {
    /// None when the case has no fixed.npy.
    pub fn read(case_dir: &Path) -> Option<Self> {
        let npy = fs::read(case_dir.join("fixed.npy")).ok()?;
        let review = case_dir.join("review");
        let listing = read(&review.join("gray.json"));
        let numbers = serde_json::from_value(listing["frames"].clone()).unwrap();
        let luma = fs::read(review.join("gray.raw")).unwrap();
        let left_out = excluded(Mask::without(&overlay_shares()).kept(), fixed_map(&npy));
        Some(Self { listing, numbers, luma, left_out })
    }

    /// The `index`th frame kept (not its frame number).
    pub fn frame(&self, index: usize) -> &[u8] {
        &self.luma[index * FRAME_PIXELS..(index + 1) * FRAME_PIXELS]
    }
}

/// A tracking run's inputs as python/retired/tests/fixtures.py --review kept them in test_out/parity/<case>/review/:
/// its tracks, the camera's readings and the countdown (from teal.json's counts).
pub struct TrackingInputs {
    pub tracks: Tracks,
    pub camera: Vec<CameraReading>,
    pub countdown: Vec<bool>,
}

impl TrackingInputs {
    /// None when the folder has no tracks.json.
    pub fn read(dir: &Path) -> Option<Self> {
        let text = fs::read_to_string(dir.join("tracks.json")).ok()?;
        let tracks = serde_json::from_str(&text).unwrap();
        let camera = serde_json::from_value(read(&dir.join("camera.json"))).unwrap();
        let teal: Vec<u32> = serde_json::from_value(read(&dir.join("teal.json"))).unwrap();
        let countdown = teal.iter().map(|&pixels| pixels >= COUNTDOWN_TEAL_PIXELS).collect();
        Some(Self { tracks, camera, countdown })
    }

    /// The core's tracking review of these inputs, as JSON, for the video and stats file a report of Python's (`want`)
    /// names, with the scenario's time limit from scenarios.json (`facts`).
    pub fn review(&self, want: &Value, facts: &Value, faint: Option<FaintSetting>) -> Value {
        let video = want["video"].as_str().unwrap();
        let stats = want["stats"].as_str().unwrap();
        let stats_text = read_lossy(Path::new(stats));
        let limit = facts["facts"][scenario_key(video).as_str()]["limit"].as_f64();
        let readings = VideoReadings { camera: &self.camera, countdown: &self.countdown };
        let kills = KillTimes::Stats { name: stats, text: &stats_text };
        let scenario = TrackScenario { limit, hitbox: None };
        let report = review_tracking(&self.tracks, kills, video, scenario, readings, None, faint).unwrap();
        serde_json::to_value(&report).unwrap()
    }
}
