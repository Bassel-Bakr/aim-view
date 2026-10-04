//! The clicking review (src/review.rs) against Python's on the same tracks: tests/fixtures.py --review writes
//! test_out/parity/<case>/review/ (tracks.json, flicks.json, measures.json, report.json). Everything that is not a
//! number must be equal; numbers within 1e-9 of Python's (relative), since the core uses plain floating point. The
//! checks are compared by their issue number, flag and numbers, not their words: the core words them in the app's terms
//! (TTK, micros, confirmation), python/review.py in its own.

use std::fs;
use std::path::PathBuf;

use aimview::review::{KillTimes, review_clicks};
use aimview::track::Tracks;
use serde_json::{json, Value};

mod common;
use common::{compare, read, rename_key, Diff};

const CASES: [&str; 2] = ["av1", "pokeball134"];

/// The numbers in a text, in order: "-12 ms (5.5%)" gives -12 and 5.5. A sign counts only at the start of a word.
fn numbers(text: &str) -> Vec<f64> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut before = ' ';
    for c in text.chars().chain([' ']) {
        let sign = (c == '+' || c == '-') && word.is_empty() && !before.is_alphanumeric();
        if c.is_ascii_digit() || sign || (c == '.' && word.chars().any(|d| d.is_ascii_digit())) {
            word.push(c);
        } else {
            if word.chars().any(|d| d.is_ascii_digit()) {
                out.push(word.trim_end_matches('.').parse().unwrap());
            }
            word.clear();
        }
        before = c;
    }
    out
}

/// Each check as its issue number, its flag and the numbers in its title, value and why: everything but the words.
fn without_words(issues: &Value) -> Value {
    let text = |i: &Value, k: &str| i[k].as_str().unwrap_or_default().to_owned();
    issues
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            let all = [text(i, "title"), text(i, "value"), text(i, "why")].join(" ");
            json!({ "issue": i["issue"], "flag": i["flag"], "numbers": numbers(&all) })
        })
        .collect()
}

/// Python's names for the measures' fields that the core names in full (src/measure.rs).
const MEASURE_RENAMES: [(&str, &str); 3] = [("n", "kill_number"), ("dir", "direction_deg"), ("corr", "corrections")];
/// Python's names for the flicks' fields that the core names in full (src/matching.rs).
const FLICK_RENAMES: [(&str, &str); 3] = [("n", "kill_number"), ("traj", "path"), ("area", "area_px")];

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
        let mut want = read(&dir.join("report.json"));
        let stats = want["stats"].as_str().unwrap();
        let Ok(stats_text) = fs::read(stats).map(|b| String::from_utf8_lossy(&b).into_owned()) else {
            eprintln!("no {stats}");
            continue;
        };
        let got = review_clicks(&tracks, KillTimes::Stats { name: stats, text: &stats_text }, want["video"].as_str().unwrap(), None, None).unwrap();
        let mut diff = Diff::default();
        let mut python_flicks = read(&dir.join("flicks.json"));
        for (python, core) in FLICK_RENAMES {
            rename_key(&mut python_flicks, &format!("[].{python}"), core);
        }
        compare("flicks.json", &serde_json::to_value(&got.flicks).unwrap(), &python_flicks, &mut diff);
        let mut measures = read(&dir.join("measures.json"));
        for (python, core) in MEASURE_RENAMES {
            rename_key(&mut measures, &format!("[].{python}"), core);
            rename_key(&mut want, &format!("flicks[].{python}"), core);
        }
        compare("measures.json", &serde_json::to_value(&got.report.flicks).unwrap(), &measures, &mut diff);
        let mut report = serde_json::to_value(&got.report).unwrap();
        report["issues"] = without_words(&report["issues"]);
        want["issues"] = without_words(&want["issues"]);
        compare("report.json", &report, &want, &mut diff);
        eprintln!("{case}: {} numbers equal within 1e-9, not to the bit", diff.close);
        for w in diff.wrong.iter().take(30) {
            eprintln!("  {w}");
        }
        assert!(diff.wrong.is_empty(), "{case}: {} differences", diff.wrong.len());
    }
}
