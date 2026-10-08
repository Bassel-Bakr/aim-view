//! The tracking review (src/tracking.rs, src/review.rs) against Python's on the same tracks and camera readings:
//! python/retired/tests/fixtures.py --review writes test_out/parity/<case>/review/ (tracks.json, camera.json,
//! teal.json, report.json). Everything that is not a number must be equal; numbers within 1e-9 of Python's (relative).

use std::fs;

mod common;
use common::{Diff, TrackingInputs, compare, parity_root, read, without_tracking_checks};

/// The parity cases of tracking runs with Python's reports. flower_crosshair (python/retired/tests/fixtures.py
/// --crosshair): flower's tracks with the valorant run's crosshair
/// boxes (av1's, from its fast turns) put into its run, so the tracking review leaves out a detector's boxes on the
/// crosshair.
const CASES: [&str; 6] = ["spectral", "flower", "pokeball5", "controlsphere", "aethercontrol", "flower_crosshair"];

/// Each case's tracking report equals Python's.
#[test]
fn tracking_review_matches_python() {
    let root = parity_root();
    let Ok(text) = fs::read_to_string(root.join("scenarios.json")) else {
        eprintln!("no scenarios.json");
        return;
    };
    let facts: serde_json::Value = serde_json::from_str(&text).unwrap();
    for case in CASES {
        let dir = root.join(case).join("review");
        let Some(inputs) = TrackingInputs::read(&dir) else {
            eprintln!("no {}", dir.display());
            continue;
        };
        let want = without_tracking_checks(read(&dir.join("report.json")));
        let got = inputs.review(&want, &facts, None);
        let mut diff = Diff::default();
        compare("report.json", &got, &want, &mut diff);
        eprintln!("{case}: {} numbers equal within 1e-9, not to the bit", diff.close);
        diff.assert_none(case);
    }
}
