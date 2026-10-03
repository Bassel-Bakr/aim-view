//! A recording's measures from the raw mouse logs (python/mouse_log.py's, or the desktop app's logger's, in the
//! layout's mouse folder): the newest log that covers the recording's run, read by the core (src/mouse.rs, as
//! python/mouse_read.py reads it).

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use aimview::mouse as reader;
use serde_json::{Value, json};

use crate::library::{Answer, Failure, Library};

#[cfg(windows)]
mod win {
    /// SYSTEMTIME.
    #[repr(C)]
    #[derive(Default)]
    pub struct SystemTime {
        pub year: u16,
        pub month: u16,
        pub weekday: u16,
        pub day: u16,
        pub hour: u16,
        pub minute: u16,
        pub second: u16,
        pub ms: u16,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub fn FileTimeToSystemTime(file_time: *const u64, system_time: *mut SystemTime) -> i32;
        pub fn SystemTimeToFileTime(system_time: *const SystemTime, file_time: *mut u64) -> i32;
        pub fn SystemTimeToTzSpecificLocalTime(zone: *const std::ffi::c_void, utc: *const SystemTime, local: *mut SystemTime) -> i32;
    }
}

/// This computer's offset from UTC (local minus UTC, seconds) at a moment (seconds since 1970), daylight saving time
/// included, as Python's local time conversions take it. 0 off Windows.
pub fn utc_offset_at(secs: f64) -> i64 {
    #[cfg(windows)]
    {
        // FILETIME: 100 ns steps since 1601
        let ft = ((secs.floor() as i64 + 11_644_473_600) * 10_000_000) as u64;
        let (mut utc, mut local, mut back) = (win::SystemTime::default(), win::SystemTime::default(), 0u64);
        // SAFETY: each call reads and writes the structs it is given
        let ok = unsafe {
            win::FileTimeToSystemTime(&ft, &mut utc) != 0
                && win::SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut local) != 0
                && win::SystemTimeToFileTime(&local, &mut back) != 0
        };
        if ok {
            return (back as i64 - ft as i64).div_euclid(10_000_000);
        }
    }
    let _ = secs;
    0
}

impl Library {
    /// The measures of a recording's run from the log in the mouse folder that covers it: {file, run, error}, or null
    /// when the recording has no stats file or no log covers its run.
    pub fn mouse_measures(&self, id: &str) -> Answer<Value> {
        let Some(stats_path) = self.stats_path(id) else { return Ok(Value::Null) };
        measures_in(&self.folders().mouse, &stats_path)
    }
}

/// The measures of a stats file's run from the newest log in `dir` that covers it (see `Library::mouse_measures`).
pub fn measures_in(dir: &Path, stats_path: &Path) -> Answer<Value> {
    let stats_name = stats_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let text = std::fs::read(stats_path).map_err(|e| Failure::from(format!("{}: {e}", stats_path.display())))?;
    let text = String::from_utf8_lossy(&text).into_owned();
    let mut logs: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|d| d.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "bin")).collect())
        .unwrap_or_default();
    logs.sort();
    logs.reverse();
    let mut first_error = None;
    for path in logs {
        let Some((wall0, end)) = span(&path) else { continue };
        let offset = utc_offset_at(wall0);
        let stats = match reader::read_stats(&stats_name, &text, offset) {
            Ok(stats) => stats,
            Err(e) => return Ok(json!({ "file": null, "run": null, "error": e })),
        };
        // the reader's own test, on the log's span: some kill within a second of it
        let (Some(first), Some(last)) = (stats.kills.first(), stats.kills.last()) else { return Ok(Value::Null) };
        if last.t < wall0 - 1.0 || first.t > end + 1.0 {
            continue;
        }
        let bytes = std::fs::read(&path).map_err(|e| Failure::from(format!("{}: {e}", path.display())))?;
        let file = path.file_name().map(|n| n.to_string_lossy().into_owned());
        let request = reader::ReadRequest { stats_name: Some(stats_name.clone()), stats_text: Some(text.clone()), options: Default::default(), utc_offset: offset };
        match reader::read(&bytes, &request) {
            reader::ReadOutcome::Run(run) => return Ok(json!({ "file": file, "run": run, "error": null })),
            reader::ReadOutcome::Error(e) => {
                first_error.get_or_insert(json!({ "file": file, "run": null, "error": e }));
            }
            reader::ReadOutcome::Summary(_) => {}
        }
    }
    Ok(first_error.unwrap_or(Value::Null))
}

/// The wall times a log covers (seconds since 1970), from its first and last bytes.
fn span(path: &Path) -> Option<(f64, f64)> {
    use std::io::{Seek, SeekFrom};
    let mut f = File::open(path).ok()?;
    let mut head = [0u8; reader::HEADER_SIZE];
    f.read_exact(&mut head).ok()?;
    let len = f.metadata().ok()?.len();
    let body = (len - reader::HEADER_SIZE as u64) / reader::RECORD_SIZE as u64 * reader::RECORD_SIZE as u64;
    let mut last = [0u8; reader::RECORD_SIZE];
    if body >= reader::RECORD_SIZE as u64 {
        f.seek(SeekFrom::Start(reader::HEADER_SIZE as u64 + body - reader::RECORD_SIZE as u64)).ok()?;
        f.read_exact(&mut last).ok()?;
    }
    reader::log_span(&head, if body > 0 { &last } else { &[] })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_are_whole_quarter_hours() {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64();
        assert_eq!(utc_offset_at(now) % 900, 0);
    }

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
        let mut other = log[..reader::HEADER_SIZE].to_vec();
        other[24..32].copy_from_slice(&(i64::from_le_bytes(log[24..32].try_into().unwrap()) - 7_200_000_000_000).to_le_bytes());
        std::fs::write(dir.join("mouse_2026-09-30_04-54-21.bin"), &other).unwrap();
        let found = measures_in(&dir, &stats);
        for f in ["mouse_2026-09-30_04-54-20.bin", "mouse_2026-09-30_04-54-21.bin"] {
            std::fs::remove_file(dir.join(f)).unwrap();
        }
        std::fs::remove_dir(&dir).unwrap();
        let found = found.unwrap_or_else(|f| panic!("{}", f.message));
        assert_eq!(found["file"], "mouse_2026-09-30_04-54-20.bin");
        assert_eq!(found["run"]["matched"], 20);
        assert_eq!(found["run"]["kills"].as_array().unwrap().len(), 20);
    }
}
