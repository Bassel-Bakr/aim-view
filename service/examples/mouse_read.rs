//! A raw mouse log read (python/mouse_read.py's command line, on the core's reader: src/mouse.rs). With a stats file it
//! measures each flick of the run and writes <log>.kills.json beside the log; without one it sums the log up.
//! cargo run -p aimview-service --release --example mouse_read -- <log.bin> [--stats "<stats csv>"] [--dpi N]
//!   [--cm360 N] [--window MS] [--start DEG_S] [--stop DEG_S] [--hold MS]

use std::path::Path;

use aimview::mouse::{self as reader, Options, ReadOutcome, ReadRequest};
use aimview_service::mouse::utc_offset_at;

const NS_PER_S: f64 = 1e9;

/// The reader's options from the command line, the defaults where an option is not given.
fn options(number: impl Fn(&str) -> Option<f64>) -> Options {
    let defaults = Options::default();
    Options {
        dpi: number("--dpi"),
        cm360: number("--cm360"),
        window: number("--window").unwrap_or(defaults.window),
        start: number("--start").unwrap_or(defaults.start),
        stop: number("--stop").unwrap_or(defaults.stop),
        hold: number("--hold").unwrap_or(defaults.hold),
    }
}

/// A file's text, with bytes that are not UTF-8 replaced.
fn read_lossy(path: &str) -> String {
    String::from_utf8_lossy(&std::fs::read(path).unwrap_or_else(|error| panic!("{path}: {error}"))).into_owned()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let value = |name: &str| args.iter().position(|arg| arg == name).and_then(|i| args.get(i + 1)).cloned();
    let number = |name: &str| {
        value(name).map(|arg| arg.parse::<f64>().unwrap_or_else(|_| panic!("{name} takes a number")))
    };
    let Some(log_path) = args.get(1).filter(|arg| !arg.starts_with("--")) else {
        eprintln!("give a log: mouse_read <log.bin> [--stats <csv>] ...");
        std::process::exit(2);
    };
    let options = options(number);
    let bytes = std::fs::read(log_path).unwrap_or_else(|error| panic!("{log_path}: {error}"));
    let utc_offset =
        reader::read_header(&bytes).map_or(0, |(_, _, start_ns)| utc_offset_at(start_ns as f64 / NS_PER_S));
    let stats = value("--stats");
    let stats_text = stats.as_deref().map(read_lossy);
    let file_name = |path: &String| {
        Path::new(path).file_name().map_or(path.clone(), |name| name.to_string_lossy().into_owned())
    };
    let stats_name = stats.as_ref().map(file_name);
    let request = ReadRequest { stats_name: stats_name.clone(), stats_text, options, utc_offset };
    match reader::read(&bytes, &request) {
        ReadOutcome::Summary(summary) => print!("{}", reader::summary_text(&summary, log_path)),
        ReadOutcome::Run(run) => {
            print!("{}", reader::run_text(&run, log_path, stats_name.as_deref().unwrap_or_default()));
            let kills_path = Path::new(log_path).with_extension("kills.json");
            let kills = reader::kills_json(&run, log_path, stats.as_deref().unwrap_or_default());
            let text = serde_json::to_string_pretty(&kills).unwrap();
            std::fs::write(&kills_path, text).unwrap_or_else(|error| panic!("{}: {error}", kills_path.display()));
            println!("wrote {}", kills_path.display());
        }
        ReadOutcome::Error(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
