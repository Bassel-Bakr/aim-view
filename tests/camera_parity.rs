//! The camera's readings (src/camera.rs) against Python's camera_motion on the same gray frames: tests/fixtures.py
//! --review writes sample frame pairs (gray.raw, gray.json) and every frame's reading (camera.json), and the
//! countdown-teal counts (teal.json, checked in the browser). The FFTs differ in rounding (rustfft against SciPy's
//! pocketfft, both in single precision), so readings must agree within 0.001 degrees.

use std::fs;
use std::path::PathBuf;

use aimview::camera::{CameraWatch, excluded};
use aimview::geometry::{H, W, overlay_shares};
use aimview::track::{Mask, Tracks};
use aimview::tracking::CameraReading;

const CASES: [&str; 3] = ["spectral", "flower", "pokeball5"];

#[test]
fn camera_readings_match_python() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_out/parity");
    for case in CASES {
        let dir = root.join(case);
        let Ok(npy) = fs::read(dir.join("fixed.npy")) else {
            eprintln!("no {}", dir.display());
            continue;
        };
        let review = dir.join("review");
        let picks: serde_json::Value = serde_json::from_str(&fs::read_to_string(review.join("gray.json")).unwrap()).unwrap();
        let stored: Vec<usize> = serde_json::from_value(picks["frames"].clone()).unwrap();
        let picks: Vec<usize> = serde_json::from_value(picks["picks"].clone()).unwrap();
        let gray = fs::read(review.join("gray.raw")).unwrap();
        let frame = |i: usize| {
            let at = stored.iter().position(|&s| s == i).unwrap() * W * H;
            &gray[at..at + W * H]
        };
        let tracks: Tracks = serde_json::from_str(&fs::read_to_string(review.join("tracks.json")).unwrap()).unwrap();
        let want: Vec<CameraReading> =
            serde_json::from_str(&fs::read_to_string(review.join("camera.json")).unwrap()).unwrap();
        let bad = excluded(Mask::without(&overlay_shares()).kept(), &npy[npy.len() - W * H..]);
        let rgb = vec![0u8; W * H * 3];
        let (mut worst, mut wrong, mut read) = (0.0f64, Vec::new(), 0);
        for &i in &picks {
            let mut watch = CameraWatch::new(&bad);
            watch.add(frame(i - 1), &rgb);
            watch.add(frame(i), &rgb);
            let near: Vec<_> = if i < tracks.frames.len() { vec![&tracks.frames[i - 1], &tracks.frames[i]] } else { vec![] };
            let got = watch.reading(&watch.shifts[1], &near);
            match (got, want[i]) {
                (None, None) => {}
                (Some(a), Some(b)) if a.2 == b.2 => {
                    worst = worst.max((a.0 - b.0).abs().max((a.1 - b.1).abs()));
                    read += 1;
                }
                (a, b) => wrong.push(format!("frame {i}: {a:?} against {b:?}")),
            }
        }
        eprintln!("{case}: {read} readings of {} frames, largest difference {worst:.2e} deg", picks.len());
        for w in &wrong {
            eprintln!("  {w}");
        }
        assert!(wrong.is_empty() && worst < 1e-3, "{case}: {} frames differ, largest difference {worst}", wrong.len());
    }
}
