//! The clicking review (src/review.rs) against Python's on the same tracks: python/retired/tests/fixtures.py --review
//! writes test_out/parity/<case>/review/ (tracks.json, flicks.json, measures.json, report.json). Everything that is not
//! a number must be equal; numbers within 1e-9 of Python's (relative), since the core uses plain floating point. The
//! checks are compared by their issue number, flag and numbers, not their words: the core words them in the app's terms
//! (TTK, micros, confirmation), python/retired/review.py in its own.

use std::fs;

use aimview::review::{KillTimes, review_clicks};
use aimview::track::Tracks;
use serde_json::{Value, json};

mod common;
use common::{Diff, compare, parity_root, read, rename_key};

/// The parity cases of clicking runs with Python's reports.
const CASES: [&str; 2] = ["av1", "pokeball134"];

/// The numbers in a text, in order: "-12 ms (5.5%)" gives -12 and 5.5. A sign counts only at the start of a word.
fn numbers(text: &str) -> Vec<f64> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut before = ' ';
    for character in text.chars().chain([' ']) {
        let has_digit = word.chars().any(|letter| letter.is_ascii_digit());
        let sign = (character == '+' || character == '-') && word.is_empty() && !before.is_alphanumeric();
        if character.is_ascii_digit() || sign || (character == '.' && has_digit) {
            word.push(character);
        } else {
            if has_digit {
                out.push(word.trim_end_matches('.').parse().unwrap());
            }
            word.clear();
        }
        before = character;
    }
    out
}

/// Each check as its issue number, its flag and the numbers in its title, value and why: everything but the words.
fn without_words(issues: &Value) -> Value {
    let text = |issue: &Value, key: &str| issue[key].as_str().unwrap_or_default().to_owned();
    issues
        .as_array()
        .unwrap()
        .iter()
        .map(|issue| {
            let all = [text(issue, "title"), text(issue, "value"), text(issue, "why")].join(" ");
            json!({ "issue": issue["issue"], "flag": issue["flag"], "numbers": numbers(&all) })
        })
        .collect()
}

/// Python's names for the measures' fields that the core names in full (src/measure.rs).
const MEASURE_RENAMES: [(&str, &str); 3] = [("n", "kill_number"), ("dir", "direction_deg"), ("corr", "corrections")];
/// Python's names for the flicks' fields that the core names in full (src/matching.rs).
const FLICK_RENAMES: [(&str, &str); 3] = [("n", "kill_number"), ("traj", "path"), ("area", "area_px")];

/// Each case's flicks, measures and report equal Python's, Python's short field names renamed and the checks' words
/// left out.
#[test]
fn clicking_review_matches_python() {
    let root = parity_root();
    for case in CASES {
        let dir = root.join(case).join("review");
        let Ok(text) = fs::read_to_string(dir.join("tracks.json")) else {
            eprintln!("no {}", dir.display());
            continue;
        };
        let tracks: Tracks = serde_json::from_str(&text).unwrap();
        let mut want = read(&dir.join("report.json"));
        let stats = want["stats"].as_str().unwrap();
        let Ok(stats_text) = fs::read(stats).map(|bytes| String::from_utf8_lossy(&bytes).into_owned()) else {
            eprintln!("no {stats}");
            continue;
        };
        let kills = KillTimes::Stats { name: stats, text: &stats_text };
        let got = review_clicks(&tracks, kills, want["video"].as_str().unwrap(), None, None).unwrap();
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
        diff.assert_none(case);
    }
}
