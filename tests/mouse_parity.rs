//! The mouse log reader (src/mouse.rs) against python/mouse_read.py on the same logs: tests/mouse_fixtures.py writes
//! test_out/parity/mouse/<case>/ (the log, its stats file, and want.json with what Python prints and writes). The
//! printed text must be the same, and every number equal to the bit.

use std::fs;
use std::path::PathBuf;

use aimview::mouse::{self, Options, ReadOutcome, ReadRequest};
use serde_json::Value;

/// Every number in `got` equal to the bit to the one in `want`, and everything else equal.
fn same(path: &str, got: &Value, want: &Value, wrong: &mut Vec<String>) {
    match (got, want) {
        (Value::Number(a), Value::Number(b)) => {
            if a.as_f64().map(f64::to_bits) != b.as_f64().map(f64::to_bits) {
                wrong.push(format!("{path}: {a} against Python's {b}"));
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            for (k, w) in b {
                same(&format!("{path}.{k}"), a.get(k).unwrap_or(&Value::Null), w, wrong);
            }
        }
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => {
            for (i, (g, w)) in a.iter().zip(b).enumerate() {
                same(&format!("{path}[{i}]"), g, w, wrong);
            }
        }
        _ if got != want => wrong.push(format!("{path}: {got} against Python's {want}")),
        _ => {}
    }
}

#[test]
fn reader_matches_python() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_out/parity/mouse");
    let Ok(cases) = fs::read_dir(&root) else {
        eprintln!("no {} (python tests/mouse_fixtures.py makes it)", root.display());
        return;
    };
    let mut checked = 0;
    for case in cases.flatten() {
        let dir = case.path();
        let Ok(text) = fs::read_to_string(dir.join("want.json")) else { continue };
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        let want: Value = serde_json::from_str(&text).unwrap();
        let log = fs::read(dir.join("log.bin")).unwrap();
        let options: Options = serde_json::from_value(want["options"].clone()).unwrap();
        let utc_offset = want["utc_offset"].as_i64().unwrap();
        let mut wrong = Vec::new();
        if want.get("log_error").is_some() {
            assert!(mouse::read_log(&log).is_err(), "{name}: read as a log");
            checked += 1;
            continue;
        }
        let summary = match mouse::read(&log, &ReadRequest { stats_name: None, stats_text: None, options: options.clone(), utc_offset }) {
            ReadOutcome::Summary(s) => s,
            _ => panic!("{name}: no summary"),
        };
        if mouse::summary_text(&summary, "log.bin") != want["summary_text"].as_str().unwrap() {
            wrong.push(format!("summary text:\n{}against Python's\n{}", mouse::summary_text(&summary, "log.bin"), want["summary_text"].as_str().unwrap()));
        }
        let s = &summary.log;
        let got = serde_json::json!({
            "travel_deg": summary.travel_deg, "deg_per_count": summary.deg_per_count, "duration": s.duration,
            "drift_ms": s.drift_ms, "wall0": s.wall0, "events": s.events, "busiest_hz": s.busiest_hz,
            "median_interval": s.median_interval, "presses": summary.presses,
        });
        same("summary", &got, &want["summary"], &mut wrong);
        if let Some(stats) = want["stats"].as_str() {
            let stats_text = String::from_utf8_lossy(&fs::read(dir.join(stats)).unwrap()).into_owned();
            let request = ReadRequest { stats_name: Some(stats.into()), stats_text: Some(stats_text), options, utc_offset };
            match (mouse::read(&log, &request), want.get("error")) {
                (ReadOutcome::Error(e), Some(w)) => same("error", &Value::String(e), w, &mut wrong),
                (ReadOutcome::Run(r), None) => {
                    let text = mouse::run_text(&r, "log.bin", stats);
                    if text != want["run_text"].as_str().unwrap() {
                        wrong.push(format!("run text:\n{text}against Python's\n{}", want["run_text"].as_str().unwrap()));
                    }
                    let w = &want["run"];
                    same("kills.json", &mouse::kills_json(&r, "log.bin", stats), &w["kills_json"], &mut wrong);
                    let got = serde_json::json!({
                        "offset_s": r.offset_s, "matched": r.matched, "spreads": r.spreads, "moving_clicks": r.moving_clicks,
                        "no_stop": r.no_stop, "corrected": r.corrected,
                    });
                    let mut w = w.clone();
                    w.as_object_mut().unwrap().remove("kills_json");
                    same("run", &got, &w, &mut wrong);
                }
                (ReadOutcome::Error(e), None) => wrong.push(format!("error {e}, where Python measured the run")),
                (_, Some(w)) => wrong.push(format!("measured, where Python stopped: {w}")),
                (ReadOutcome::Summary(_), None) => wrong.push("a summary for a run".into()),
            }
        }
        for w in wrong.iter().take(20) {
            eprintln!("{name}: {w}");
        }
        assert!(wrong.is_empty(), "{name}: {} differences", wrong.len());
        checked += 1;
    }
    eprintln!("{checked} logs read as Python reads them");
}
