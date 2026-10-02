//! The clicking review (src/review.rs) against Python's on the same tracks: tests/fixtures.py --review writes
//! test_out/parity/<case>/review/ (tracks.json, flicks.json, measures.json, report.json). Everything that is not a
//! number must be equal; numbers within 1e-9 of Python's (relative), since the core uses plain floating point.

use std::fs;
use std::path::PathBuf;

use serde_json::Value;

use aimview::review::review_clicks;
use aimview::track::Tracks;

const CASES: [&str; 2] = ["av1", "pokeball134"];

/// Where two JSON values differ (paths), and how many numbers were equal only within the tolerance.
#[derive(Default)]
struct Diff {
    wrong: Vec<String>,
    close: usize,
}

fn compare(path: &str, got: &Value, want: &Value, diff: &mut Diff) {
    match (got, want) {
        (Value::Number(a), Value::Number(b)) => {
            let (a, b) = (a.as_f64().unwrap(), b.as_f64().unwrap());
            if a != b {
                if (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1e-3) {
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
        (Value::Object(a), Value::Object(b)) => {
            for k in a.keys().chain(b.keys().filter(|k| !a.contains_key(*k))) {
                match (a.get(k), b.get(k)) {
                    (Some(x), Some(y)) => compare(&format!("{path}.{k}"), x, y, diff),
                    (x, y) => diff.wrong.push(format!("{path}.{k}: {x:?} against {y:?}")),
                }
            }
        }
        _ if got == want => {}
        _ => diff.wrong.push(format!("{path}: {got} against {want}")),
    }
}

fn read(path: &PathBuf) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

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
