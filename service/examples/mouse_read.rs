//! A raw mouse log read (python/mouse_read.py's command line, on the core's reader: src/mouse.rs). With a stats file it
//! measures each flick of the run and writes <log>.kills.json beside the log; without one it sums the log up.
//! cargo run -p aimview-service --release --example mouse_read -- <log.bin> [--stats "<stats csv>"] [--dpi N]
//!   [--cm360 N] [--window MS] [--start DEG_S] [--stop DEG_S] [--hold MS]

use std::path::Path;

use aimview::mouse::{self as reader, Options, ReadOutcome, ReadRequest};
use aimview_service::mouse::utc_offset_at;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let value = |name: &str| a.iter().position(|v| v == name).and_then(|i| a.get(i + 1)).cloned();
    let number = |name: &str| value(name).map(|v| v.parse::<f64>().unwrap_or_else(|_| panic!("{name} takes a number")));
    let Some(log_path) = a.get(1).filter(|v| !v.starts_with("--")) else {
        eprintln!("give a log: mouse_read <log.bin> [--stats <csv>] ...");
        std::process::exit(2);
    };
    let d = Options::default();
    let options = Options {
        dpi: number("--dpi"),
        cm360: number("--cm360"),
        window: number("--window").unwrap_or(d.window),
        start: number("--start").unwrap_or(d.start),
        stop: number("--stop").unwrap_or(d.stop),
        hold: number("--hold").unwrap_or(d.hold),
    };
    let bytes = std::fs::read(log_path).unwrap_or_else(|e| panic!("{log_path}: {e}"));
    let utc_offset = reader::read_header(&bytes).map_or(0, |(_, _, ns0)| utc_offset_at(ns0 as f64 / 1e9));
    let stats = value("--stats");
    let stats_text = stats.as_ref().map(|s| String::from_utf8_lossy(&std::fs::read(s).unwrap_or_else(|e| panic!("{s}: {e}"))).into_owned());
    let stats_name = stats.as_ref().map(|s| Path::new(s).file_name().map_or(s.clone(), |n| n.to_string_lossy().into_owned()));
    let request = ReadRequest { stats_name: stats_name.clone(), stats_text, options, utc_offset };
    match reader::read(&bytes, &request) {
        ReadOutcome::Summary(s) => print!("{}", reader::summary_text(&s, log_path)),
        ReadOutcome::Run(r) => {
            print!("{}", reader::run_text(&r, log_path, stats_name.as_deref().unwrap_or_default()));
            let js = Path::new(log_path).with_extension("kills.json");
            let text = serde_json::to_string_pretty(&reader::kills_json(&r, log_path, stats.as_deref().unwrap_or_default())).unwrap();
            std::fs::write(&js, text).unwrap_or_else(|e| panic!("{}: {e}", js.display()));
            println!("wrote {}", js.display());
        }
        ReadOutcome::Error(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
