//! `scenario::facts` against Python's `scenario_facts` and `target_counts` over every scenario file on this computer
//! (test_out/parity/scenarios.json: the files in Python's order, and Python's facts by lower-case name). Later
//! files win, as in Python's dict.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use aimview::scenario::{Facts, facts};
use serde_json::Value;

mod common;
use common::parity_root;

/// How many differing scenarios the test prints before it fails.
const SHOWN_SCENARIOS: usize = 5;

/// Every scenario file's facts equal Python's, on the facts Python reads.
#[test]
fn scenario_facts_match_python() {
    let path = parity_root().join("scenarios.json");
    let Ok(text) = fs::read_to_string(&path) else {
        eprintln!("no {}: the fixtures are frozen (python/retired/tests/fixtures.py made them)", path.display());
        return;
    };
    let fixture: Value = serde_json::from_str(&text).unwrap();
    let mut got: BTreeMap<String, Facts> = BTreeMap::new();
    for scenario_path in fixture["paths"].as_array().unwrap() {
        let scenario_path = Path::new(scenario_path.as_str().unwrap());
        let bytes = fs::read(scenario_path).unwrap();
        let name = scenario_path.file_stem().unwrap().to_string_lossy().to_lowercase();
        got.insert(name, facts(&String::from_utf8_lossy(&bytes)));
    }
    let want = fixture["facts"].as_object().unwrap();
    assert_eq!(got.len(), want.len(), "scenarios");
    let mut wrong = 0;
    for (name, python) in want {
        let mut core = serde_json::to_value(&got[name]).unwrap();
        // only the facts Python reads: a fact the core adds is not compared
        core.as_object_mut().unwrap().retain(|key, _| python.get(key).is_some());
        if &core != python {
            if wrong < SHOWN_SCENARIOS {
                eprintln!("{name}: rust {core} python {python}");
            }
            wrong += 1;
        }
    }
    assert_eq!(wrong, 0, "{wrong} of {} scenarios differ", want.len());
    eprintln!("{} scenarios equal", want.len());
}
