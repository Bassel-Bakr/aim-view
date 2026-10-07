//! `python::hypot` against CPython's `math.hypot` on 20,000 random pairs and a few edge cases, to the bit
//! (test_out/parity/hypot.json: [x, y, math.hypot(x, y)] rows, made by python/retired/tests/fixtures.py's hypot cases),
//! and the KovOBS overlay's boxes against Python's (test_out/parity/overlay.json).

use std::fs;

use aimview::python::hypot;

mod common;
use common::parity_root;

/// How many differing pairs the test prints before it fails.
const SHOWN_PAIRS: usize = 3;

/// Every stored pair's `hypot` equals CPython's to the bit.
#[test]
fn hypot_matches_cpython() {
    let path = parity_root().join("hypot.json");
    let Ok(text) = fs::read_to_string(&path) else {
        eprintln!("no {}: the fixtures are frozen (python/retired/tests/fixtures.py made them)", path.display());
        return;
    };
    let rows: Vec<[f64; 3]> = serde_json::from_str(&text).unwrap();
    let wrong: Vec<&[f64; 3]> = rows.iter().filter(|[x, y, length]| hypot(*x, *y) != *length).collect();
    for [x, y, length] in wrong.iter().take(SHOWN_PAIRS) {
        eprintln!("hypot({x}, {y}): rust {} python {length}", hypot(*x, *y));
    }
    assert!(wrong.is_empty(), "{} of {} differ", wrong.len(), rows.len());
}

/// The KovOBS overlay's shares of the frame equal Python's to the bit.
#[test]
fn kovobs_overlay_matches_python() {
    let path = parity_root().join("overlay.json");
    let Ok(text) = fs::read_to_string(&path) else {
        eprintln!("no {}: the fixtures are frozen (python/retired/tests/fixtures.py made them)", path.display());
        return;
    };
    let want: Vec<[f64; 4]> = serde_json::from_str(&text).unwrap();
    assert_eq!(want, aimview::geometry::overlay_shares());
}
