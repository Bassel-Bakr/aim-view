//! The clicking review (src/review.rs) against Python's on the same tracks: tests/fixtures.py --review writes
//! test_out/parity/<case>/review/ (tracks.json, flicks.json, measures.json, report.json). Everything that is not a
//! number must be equal; numbers within 1e-9 of Python's (relative), since the core uses plain floating point.

use std::fs;
use std::path::PathBuf;

use aimview::review::review_clicks;
use aimview::track::Tracks;

mod common;
use common::{compare, read, Diff};

const CASES: [&str; 2] = ["av1", "pokeball134"];

#[test]
fn clicking_review_matches_python() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_out/parity");
    for case in CASES {
        let dir = root.join(case).join("review");
        let Ok(text) = fs::read_to_string(dir.join("tracks.json")) else {
            eprintln!("no {}", dir.display());
            continue;
        };
        let tracks: Tracks = serde_json::from_str(&text).unwrap();
        let want = read(&dir.join("report.json"));
        let stats = want["stats"].as_str().unwrap();
        let Ok(stats_text) = fs::read(stats).map(|b| String::from_utf8_lossy(&b).into_owned()) else {
            eprintln!("no {stats}");
            continue;
        };
        let got = review_clicks(&tracks, &stats_text, want["video"].as_str().unwrap(), stats, None).unwrap();
        let mut diff = Diff::default();
        compare("flicks.json", &serde_json::to_value(&got.flicks).unwrap(), &read(&dir.join("flicks.json")), &mut diff);
        compare("measures.json", &serde_json::to_value(&got.report.flicks).unwrap(), &read(&dir.join("measures.json")), &mut diff);
        compare("report.json", &serde_json::to_value(&got.report).unwrap(), &want, &mut diff);
        eprintln!("{case}: {} numbers equal within 1e-9, not to the bit", diff.close);
        for w in diff.wrong.iter().take(30) {
            eprintln!("  {w}");
        }
        assert!(diff.wrong.is_empty(), "{case}: {} differences", diff.wrong.len());
    }
}
