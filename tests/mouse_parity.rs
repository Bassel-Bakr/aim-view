//! The mouse log reader (src/mouse.rs) against python/mouse_read.py on the same logs: tests/mouse_fixtures.py writes
//! test_out/parity/mouse/<case>/ (the log, its stats file, and want.json with what Python prints and writes). The
//! printed text must be the same, and every number equal to the bit.

use std::fs;
use std::path::Path;

use aimview::mouse::{self, LogSummary, Options, ReadOutcome, ReadRequest};
use serde_json::{Value, json};

mod common;
use common::{parity_root, read_lossy};

/// The name the texts give the log.
const LOG_NAME: &str = "log.bin";
/// How many differences a log prints before it fails.
const SHOWN_DIFFERENCES: usize = 20;

/// Every number in `got` equal to the bit to the one in `want`, and everything else equal.
fn same(path: &str, got: &Value, want: &Value, wrong: &mut Vec<String>) {
    match (got, want) {
        (Value::Number(a), Value::Number(b)) => {
            if a.as_f64().map(f64::to_bits) != b.as_f64().map(f64::to_bits) {
                wrong.push(format!("{path}: {a} against Python's {b}"));
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            for (key, wanted) in b {
                same(&format!("{path}.{key}"), a.get(key).unwrap_or(&Value::Null), wanted, wrong);
            }
        }
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => {
            for (i, (x, y)) in a.iter().zip(b).enumerate() {
                same(&format!("{path}[{i}]"), x, y, wrong);
            }
        }
        _ if got != want => wrong.push(format!("{path}: {got} against Python's {want}")),
        _ => {}
    }
}

/// The log's summary against Python's: its printed text and its numbers.
fn compare_summary(summary: &LogSummary, want: &Value, wrong: &mut Vec<String>) {
    let text = mouse::summary_text(summary, LOG_NAME);
    let want_text = want["summary_text"].as_str().unwrap();
    if text != want_text {
        wrong.push(format!("summary text:\n{text}against Python's\n{want_text}"));
    }
    let log = &summary.log;
    let got = json!({
        "travel_deg": summary.travel_deg, "deg_per_count": summary.deg_per_count, "duration": log.duration,
        "drift_ms": log.drift_ms, "wall0": log.wall0, "events": log.events, "busiest_hz": log.busiest_hz,
        "median_interval": log.median_interval, "presses": summary.presses,
    });
    same("summary", &got, &want["summary"], wrong);
}

/// The log read with its stats file against Python's: the same error, or the same run (its text, kills.json and
/// numbers).
fn compare_run(outcome: ReadOutcome, stats: &str, want: &Value, wrong: &mut Vec<String>) {
    match (outcome, want.get("error")) {
        (ReadOutcome::Error(error), Some(want_error)) => same("error", &Value::String(error), want_error, wrong),
        (ReadOutcome::Run(run), None) => {
            let text = mouse::run_text(&run, LOG_NAME, stats);
            let want_text = want["run_text"].as_str().unwrap();
            if text != want_text {
                wrong.push(format!("run text:\n{text}against Python's\n{want_text}"));
            }
            let want_run = &want["run"];
            same("kills.json", &mouse::kills_json(&run, LOG_NAME, stats), &want_run["kills_json"], wrong);
            let got = json!({
                "offset_s": run.offset_s, "matched": run.matched, "spreads": run.spreads,
                "moving_clicks": run.moving_clicks, "no_stop": run.no_stop, "corrected": run.corrected,
            });
            let mut want_run = want_run.clone();
            want_run.as_object_mut().unwrap().remove("kills_json");
            same("run", &got, &want_run, wrong);
        }
        (ReadOutcome::Error(error), None) => wrong.push(format!("error {error}, where Python measured the run")),
        (_, Some(want_error)) => wrong.push(format!("measured, where Python stopped: {want_error}")),
        (ReadOutcome::Summary(_), None) => wrong.push("a summary for a run".into()),
    }
}

/// Reads one case's log as the app does and fails on any difference from Python's.
fn check_case(dir: &Path, name: &str, want: &Value) {
    let log = fs::read(dir.join(LOG_NAME)).unwrap();
    let options: Options = serde_json::from_value(want["options"].clone()).unwrap();
    let utc_offset = want["utc_offset"].as_i64().unwrap();
    let mut wrong = Vec::new();
    if want.get("log_error").is_some() {
        assert!(mouse::read_log(&log).is_err(), "{name}: read as a log");
        return;
    }
    let request = ReadRequest { stats_name: None, stats_text: None, options: options.clone(), utc_offset };
    let summary = match mouse::read(&log, &request) {
        ReadOutcome::Summary(summary) => summary,
        _ => panic!("{name}: no summary"),
    };
    compare_summary(&summary, want, &mut wrong);
    if let Some(stats) = want["stats"].as_str() {
        let stats_text = read_lossy(&dir.join(stats));
        let request = ReadRequest { stats_name: Some(stats.into()), stats_text: Some(stats_text), options, utc_offset };
        compare_run(mouse::read(&log, &request), stats, want, &mut wrong);
    }
    for difference in wrong.iter().take(SHOWN_DIFFERENCES) {
        eprintln!("{name}: {difference}");
    }
    assert!(wrong.is_empty(), "{name}: {} differences", wrong.len());
}

#[test]
fn reader_matches_python() {
    let root = parity_root().join("mouse");
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
        check_case(&dir, &name, &want);
        checked += 1;
    }
    eprintln!("{checked} logs read as Python reads them");
}
