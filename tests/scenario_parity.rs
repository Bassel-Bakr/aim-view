//! `scenario::facts` against Python's `scenario_facts` and `target_counts` over every scenario file on this computer
//! (test_out/parity/scenarios.json: the files in Python's order, and Python's facts by lower-case name). Later
//! files win, as in Python's dict.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use aimview::scenario::{Facts, facts};
use serde_json::Value;

#[test]
fn scenario_facts_match_python() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_out/parity/scenarios.json");
    let Ok(text) = fs::read_to_string(&path) else {
        eprintln!("no {}: run python tests/fixtures.py --scenarios", path.display());
        return;
    };
    let fixture: Value = serde_json::from_str(&text).unwrap();
    let mut got: BTreeMap<String, Facts> = BTreeMap::new();
    for p in fixture["paths"].as_array().unwrap() {
        let p = Path::new(p.as_str().unwrap());
        let bytes = fs::read(p).unwrap();
        let name = p.file_stem().unwrap().to_string_lossy().to_lowercase();
        got.insert(name, facts(&String::from_utf8_lossy(&bytes)));
    }
    let want = fixture["facts"].as_object().unwrap();
    assert_eq!(got.len(), want.len(), "scenarios");
    let mut wrong = 0;
    for (name, w) in want {
        let mut g = serde_json::to_value(&got[name]).unwrap();
        // only the facts Python reads: a fact the core adds is not compared
        g.as_object_mut().unwrap().retain(|k, _| w.get(k).is_some());
        if &g != w {
            if wrong < 5 {
                eprintln!("{name}: rust {g} python {w}");
            }
            wrong += 1;
        }
    }
    assert_eq!(wrong, 0, "{wrong} of {} scenarios differ", want.len());
    eprintln!("{} scenarios equal", want.len());
}
