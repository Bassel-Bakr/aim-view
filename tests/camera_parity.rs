//! The camera's readings (src/camera.rs) against Python's camera_motion on the same gray frames:
//! python/retired/tests/fixtures.py --review writes sample frame pairs (gray.raw, gray.json) and every frame's reading
//! (camera.json), and the countdown-teal counts (teal.json, checked in the browser). The FFTs differ in rounding
//! (rustfft against SciPy's pocketfft, both in single precision), so readings must agree within 0.001 degrees.

use std::fs;
use std::path::Path;

use aimview::camera::CameraWatch;
use aimview::geometry::{H, W};
use aimview::track::Tracks;
use aimview::tracking::CameraReading;

mod common;
use common::{GrayFrames, parity_root};

const CASES: [&str; 3] = ["spectral", "flower", "pokeball5"];
/// The largest difference allowed between a reading and Python's, in degrees.
const MAX_DIFFERENCE_DEG: f64 = 1e-3;

/// How a case's readings compare with Python's: how many were read, the largest difference (degrees), and the frames
/// where one side read and the other did not, or the tiles that agreed differ.
#[derive(Default)]
struct Agreement {
    readings: usize,
    worst_deg: f64,
    wrong: Vec<String>,
}

/// Reads the camera's turn into each picked frame from it and the frame before, as the review reads it, and compares
/// it with Python's reading. None when the case has no fixtures.
fn compare_case(dir: &Path) -> Option<(Agreement, usize)> {
    let frames = GrayFrames::read(dir)?;
    let review = dir.join("review");
    let picks: Vec<usize> = serde_json::from_value(frames.listing["picks"].clone()).unwrap();
    let numbered = |number: usize| frames.frame(frames.numbers.iter().position(|&stored| stored == number).unwrap());
    let json_file = |name: &str| fs::read_to_string(review.join(name)).unwrap();
    let tracks: Tracks = serde_json::from_str(&json_file("tracks.json")).unwrap();
    let want: Vec<CameraReading> = serde_json::from_str(&json_file("camera.json")).unwrap();
    let rgb = vec![0u8; W * H * 3];
    let mut agreement = Agreement::default();
    for &i in &picks {
        let mut watch = CameraWatch::new(&frames.left_out);
        watch.add(numbered(i - 1), &rgb);
        watch.add(numbered(i), &rgb);
        let near = if i < tracks.frames.len() { vec![&tracks.frames[i - 1], &tracks.frames[i]] } else { vec![] };
        let got = watch.reading(&watch.shifts[1], &near);
        match (got, want[i]) {
            (None, None) => {}
            (Some(a), Some(b)) if a.2 == b.2 => {
                agreement.worst_deg = agreement.worst_deg.max((a.0 - b.0).abs().max((a.1 - b.1).abs()));
                agreement.readings += 1;
            }
            (a, b) => agreement.wrong.push(format!("frame {i}: {a:?} against {b:?}")),
        }
    }
    Some((agreement, picks.len()))
}

#[test]
fn camera_readings_match_python() {
    for case in CASES {
        let dir = parity_root().join(case);
        let Some((Agreement { readings, worst_deg, wrong }, picked)) = compare_case(&dir) else {
            eprintln!("no {}", dir.display());
            continue;
        };
        eprintln!("{case}: {readings} readings of {picked} frames, largest difference {worst_deg:.2e} deg");
        for difference in &wrong {
            eprintln!("  {difference}");
        }
        let differ = wrong.len();
        assert!(
            wrong.is_empty() && worst_deg < MAX_DIFFERENCE_DEG,
            "{case}: {differ} frames differ, largest difference {worst_deg}"
        );
    }
}
