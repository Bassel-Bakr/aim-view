//! `python::hypot` against CPython's `math.hypot` on 20,000 random pairs and a few edge cases, to the bit
//! (test_out/parity/hypot.json: [x, y, math.hypot(x, y)] rows, made by tests/fixtures.py's hypot cases).

use std::fs;
use std::path::PathBuf;

#[test]
fn hypot_matches_cpython() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_out/parity/hypot.json");
    let Ok(text) = fs::read_to_string(&path) else {
        eprintln!("no {}: run python tests/fixtures.py --hypot", path.display());
        return;
    };
    let rows: Vec<[f64; 3]> = serde_json::from_str(&text).unwrap();
    let wrong: Vec<&[f64; 3]> = rows.iter().filter(|[x, y, h]| aimview::python::hypot(*x, *y) != *h).collect();
    for [x, y, h] in wrong.iter().take(3) {
        eprintln!("hypot({x}, {y}): rust {} python {h}", aimview::python::hypot(*x, *y));
    }
    assert!(wrong.is_empty(), "{} of {} differ", wrong.len(), rows.len());
}

#[test]
fn kovobs_overlay_matches_python() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_out/parity/overlay.json");
    let Ok(text) = fs::read_to_string(&path) else {
        eprintln!("no {}: run python tests/fixtures.py --hypot", path.display());
        return;
    };
    let want: Vec<[f64; 4]> = serde_json::from_str(&text).unwrap();
    assert_eq!(aimview::geometry::overlay_shares(), want);
}
