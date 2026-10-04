//! The review after the detector, replayed from the parts the native review kept, without the video: each run's track
//! part (the detector's boxes, the pop-up areas' looks) and watch part (the camera's tile shifts, the countdown, the
//! HUD's glyphs) joined as the review joins them (keep, the pop-ups, link, the camera's readings, the HUD's reading),
//! then the report worked out as the service does (matching, measures, summary, checks). Every output must equal the
//! native review's, byte for byte: the byte-compare's baseline (BENCH.md, Correctness; `NATIVE`/<video>/: tracks.json,
//! readings.json, hud.json, report.json; no_stats/ without the stats file). A change after the detector is checked
//! here in about a second a video, not with a whole review. The parts are in test_out/baselines/parts/<video>/, kept
//! once by the track example's `--parts` (service/examples/track.rs; saved.txt there): setup.json, fixed.bin,
//! run<k>_track.json, run<k>_watch.json and detector.txt. They change only with what comes before the join (decoding,
//! the conversion, the detector, the watches' readings of each frame): keep them again then.

use std::fs;
use std::path::{Path, PathBuf};

use aimview::review::review_json;
use aimview::session::{Review, Setup, WatchPart};
use aimview::tracker::TrackPart;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

mod common;
use common::{Diff, compare, read as read_json};

/// The byte-compare's baseline: move it with BENCH.md's.
const NATIVE: &str = "test_out/baselines/9b04bc5/native";
const PARTS: &str = "test_out/baselines/parts";
const AV1: &str = "1wall 2targets xsmall - valorant - 558.46 - 2026.10.01-16.23.04";
const FLOWER: &str = "Flower Easier - 4801 - 2026.09.20-02.33.56";

/// The byte-compare's cases (test_out/baselines/native_compare.py): the video, its stats file (None: the review
/// without one), and the folder of the outputs in the video's.
const CASES: [(&str, Option<&str>, &str); 3] = [
    (AV1, Some("test_out/parity/av1/review/stats.csv"), ""),
    (AV1, None, "no_stats"),
    (FLOWER, Some("test_out/parity/flower/review/stats.csv"), ""),
];

const FILES: [&str; 4] = ["tracks.json", "readings.json", "hud.json", "report.json"];

fn read<T: DeserializeOwned>(path: &Path) -> T {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The review's tracks, readings and HUD reading, as the track example writes them, from its kept parts.
fn join(parts: &Path) -> [Vec<u8>; 3] {
    let setup: Setup = read(&parts.join("setup.json"));
    let fixed = fs::read(parts.join("fixed.bin")).unwrap();
    let detector = fs::read_to_string(parts.join("detector.txt")).unwrap();
    let review = Review::new(setup).unwrap();
    let mut joining = review.joining(&fixed);
    for run in 0..review.runs().len() {
        let track: TrackPart = read(&parts.join(format!("run{run}_track.json")));
        let watch: WatchPart = read(&parts.join(format!("run{run}_watch.json")));
        joining.add(track, watch);
    }
    let joined = joining.finish(detector).unwrap();
    let tracks = serde_json::to_vec(&joined.tracks).unwrap();
    [tracks, serde_json::to_vec(&joined.readings).unwrap(), serde_json::to_vec(&joined.hud).unwrap()]
}

/// The report as the service works it out from the three files (service/src/report.rs, `work_out`, as the track
/// example calls it: no run marks, no scenario facts, no cut-off).
fn report(outputs: &[Vec<u8>; 3], video: &str, stats: Option<&Path>) -> Vec<u8> {
    let value = |b: &[u8]| serde_json::from_slice::<Value>(b).unwrap();
    let (tracks, readings, hud) = (value(&outputs[0]), value(&outputs[1]), value(&outputs[2]));
    let stats_text = stats.map_or(String::new(), |p| String::from_utf8_lossy(&fs::read(p).unwrap()).into_owned());
    let name = |p: &Path| p.file_name().unwrap().to_string_lossy().into_owned();
    let request = json!({
        "tracks": tracks,
        "statsText": stats_text,
        "video": format!("{video}.mp4"),
        "stats": stats.map(name).unwrap_or_default(),
        "hud": hud,
        "run": null,
        "tracking": false,
        "limit": null,
        "reload": null,
        "camera": readings["camera"],
        "countdown": readings["countdown"],
        "faint": null,
    });
    let outcome: Value = serde_json::from_slice(&review_json(&serde_json::to_vec(&request).unwrap())).unwrap();
    assert!(outcome["error"].is_null(), "{video}: {}", outcome["error"]);
    serde_json::to_vec(&outcome["report"]).unwrap()
}

#[test]
fn replayed_reviews_equal_the_native_ones() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut checked = 0;
    let mut wrong = Vec::new();
    for (video, stats, sub) in CASES {
        let dir = root.join(NATIVE).join(video);
        let parts = root.join(PARTS).join(video);
        if !parts.join("setup.json").exists() {
            eprintln!("no {}: run the track example with --parts (BENCH.md, Correctness)", parts.display());
            continue;
        }
        let [tracks, readings, hud] = join(&parts);
        let stats = stats.map(|s| root.join(s));
        let report = report(&[tracks.clone(), readings.clone(), hud.clone()], video, stats.as_deref());
        for (file, got) in FILES.iter().zip([tracks, readings, hud, report]) {
            let path = dir.join(sub).join(file);
            let want = fs::read(&path).unwrap();
            checked += 1;
            if got == want {
                continue;
            }
            // where they differ, as JSON paths
            let mut diff = Diff::default();
            compare(file, &serde_json::from_slice(&got).unwrap(), &read_json(&path), &mut diff);
            let (short, sizes) = (&video[..20], (got.len(), want.len()));
            eprintln!("{short} {sub} {file}: {sizes:?} bytes, {} numbers equal only within 1e-9", diff.close);
            for w in diff.wrong.iter().take(10) {
                eprintln!("  {w}");
            }
            wrong.push(format!("{} {sub} {file}", &video[..20]));
        }
        eprintln!("{} {}: replayed", &video[..20], if sub.is_empty() { "stats" } else { sub });
    }
    assert!(wrong.is_empty(), "differ from the native review: {wrong:?}");
    eprintln!("{checked} files equal to the native review's, byte for byte");
}
