//! A recording's measures from the raw mouse logs (python/mouse_log.py's, or the desktop app's logger's, in the
//! layout's mouse folder): the newest log that covers the recording's run, read by the core (src/mouse.rs, as
//! python/mouse_read.py reads it). In: the recording's stats file and the mouse folder's logs. Out: the run's measures,
//! which /api/mouse answers.

use std::io::Read;
use std::path::{Path, PathBuf};

use aimview::mouse as reader;
use serde_json::{Value, json};

use crate::disk::File;
use crate::library::{Answer, Failure, Library};

/// How near the log's span a kill of the run must be, in seconds (the reader's own test).
const KILL_SLACK_S: f64 = 1.0;

impl Library {
    /// The measures of a recording's run from the log in the mouse folder that covers it: {file, run, error}, or null
    /// when the recording has no stats file or no log covers its run.
    pub fn mouse_measures(&self, id: &str) -> Answer<Value> {
        let Some(stats_path) = self.stats_path(id) else { return Ok(Value::Null) };
        measures_in(&self.folders().mouse, &stats_path, &self.stats_bytes(&stats_path)?)
    }
}

/// A file's name, or None.
fn file_name(path: &Path) -> Option<String> {
    path.file_name().map(|name| name.to_string_lossy().into_owned())
}

/// A file's bytes, or a failure that names it.
fn read_named(path: &Path) -> Answer<Vec<u8>> {
    crate::disk::read(path).map_err(|error| Failure::from(format!("{}: {error}", path.display())))
}

/// The mouse logs (.bin) in `dir`, newest first (their names start with their time).
fn logs_newest_first(dir: &Path) -> Vec<PathBuf> {
    let mut logs: Vec<PathBuf> = crate::disk::read_dir(dir)
        .map(|listing| {
            let paths = listing.flatten().map(|entry| entry.path());
            paths.filter(|path| path.extension().is_some_and(|extension| extension == "bin")).collect()
        })
        .unwrap_or_default();
    logs.sort();
    logs.reverse();
    logs
}

/// The measures of a stats file's run (its path and text) from the newest log in `dir` that covers it (see
/// `Library::mouse_measures`).
pub fn measures_in(dir: &Path, stats_path: &Path, stats_bytes: &[u8]) -> Answer<Value> {
    let stats_name = file_name(stats_path).unwrap_or_default();
    let text = String::from_utf8_lossy(stats_bytes).into_owned();
    let mut first_error = None;
    for path in logs_newest_first(dir) {
        let Some((log_start, log_end)) = span(&path) else { continue };
        let offset = crate::disk::utc_offset_at(log_start);
        let stats = match reader::read_stats(&stats_name, &text, offset) {
            Ok(stats) => stats,
            Err(error) => return Ok(json!({ "file": null, "run": null, "error": error })),
        };
        let (Some(first), Some(last)) = (stats.kills.first(), stats.kills.last()) else { return Ok(Value::Null) };
        if last.epoch_s < log_start - KILL_SLACK_S || first.epoch_s > log_end + KILL_SLACK_S {
            continue;
        }
        let bytes = read_named(&path)?;
        let file = file_name(&path);
        let request = reader::ReadRequest {
            stats_name: Some(stats_name.clone()),
            stats_text: Some(text.clone()),
            options: Default::default(),
            utc_offset: offset,
        };
        match reader::read(&bytes, &request) {
            reader::ReadOutcome::Run(run) => return Ok(json!({ "file": file, "run": run, "error": null })),
            reader::ReadOutcome::Error(error) => {
                first_error.get_or_insert(json!({ "file": file, "run": null, "error": error }));
            }
            reader::ReadOutcome::Summary(_) => {}
        }
    }
    Ok(first_error.unwrap_or(Value::Null))
}

/// The wall times a log covers (seconds since 1970), from its first and last bytes.
fn span(path: &Path) -> Option<(f64, f64)> {
    use std::io::{Seek, SeekFrom};
    let (header_bytes, record_bytes) = (reader::HEADER_SIZE as u64, reader::RECORD_SIZE as u64);
    let mut file = File::open(path).ok()?;
    let mut head = [0u8; reader::HEADER_SIZE];
    file.read_exact(&mut head).ok()?;
    let len = file.metadata().ok()?.len();
    // the whole records after the header (a log cut off mid-record ends with part of one)
    let records = (len - header_bytes) / record_bytes * record_bytes;
    let mut last = [0u8; reader::RECORD_SIZE];
    if records >= record_bytes {
        file.seek(SeekFrom::Start(header_bytes + records - record_bytes)).ok()?;
        file.read_exact(&mut last).ok()?;
    }
    reader::log_span(&head, if records > 0 { &last } else { &[] })
}

/// The log search.
#[cfg(test)]
mod tests {
    use super::*;

    /// Of two logs, the one whose span covers the stats file's kills is read, and all 20 kills match (skipped when
    /// the self-test's files are not made).
    #[test]
    fn finds_the_log_that_covers_the_run() {
        // the self-test's run (tests/mouse_fixtures.py), beside a log of another time
        let case = Path::new(env!("CARGO_MANIFEST_DIR")).join("../test_out/parity/mouse/selftest");
        let stats = case.join("Selftest - Challenge - 2026.09.30-04.54.28 Stats.csv");
        let Ok(log) = std::fs::read(case.join("log.bin")) else {
            eprintln!("no {} (python tests/mouse_fixtures.py makes it)", case.display());
            return;
        };
        let dir = std::env::temp_dir().join(format!("aimview-mouse-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("mouse_2026-09-30_04-54-20.bin"), &log).unwrap();
        // the other log: the header alone, its start time (nanoseconds, bytes 24 to 32) two hours earlier
        let mut other = log[..reader::HEADER_SIZE].to_vec();
        let start_ns = i64::from_le_bytes(log[24..32].try_into().unwrap());
        other[24..32].copy_from_slice(&(start_ns - 7_200_000_000_000).to_le_bytes());
        std::fs::write(dir.join("mouse_2026-09-30_04-54-21.bin"), &other).unwrap();
        let found = measures_in(&dir, &stats, &std::fs::read(&stats).unwrap());
        for name in ["mouse_2026-09-30_04-54-20.bin", "mouse_2026-09-30_04-54-21.bin"] {
            std::fs::remove_file(dir.join(name)).unwrap();
        }
        std::fs::remove_dir(&dir).unwrap();
        let found = found.unwrap_or_else(|failure| panic!("{}", failure.message));
        assert_eq!(found["file"], "mouse_2026-09-30_04-54-20.bin");
        assert_eq!(found["run"]["matched"], 20);
        assert_eq!(found["run"]["kills"].as_array().unwrap().len(), 20);
    }
}
