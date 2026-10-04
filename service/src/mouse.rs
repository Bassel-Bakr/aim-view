//! A recording's measures from the raw mouse logs (python/mouse_log.py's, or the desktop app's logger's, in the
//! layout's mouse folder): the newest log that covers the recording's run, read by the core (src/mouse.rs, as
//! python/mouse_read.py reads it). In: the recording's stats file and the mouse folder's logs. Out: the run's measures,
//! which /api/mouse answers. Also this computer's offset from UTC, for the logs' and the stats files' clocks.

use std::io::Read;
use std::path::{Path, PathBuf};

use aimview::mouse as reader;
use serde_json::{Value, json};

use crate::disk::File;
use crate::library::{Answer, Failure, Library};

/// How near the log's span a kill of the run must be, in seconds (the reader's own test).
const KILL_SLACK_S: f64 = 1.0;

#[cfg(all(windows, feature = "native"))]
mod win {
    /// Seconds from 1601 (FILETIME's start) to 1970 (Unix time's).
    pub const FILETIME_TO_UNIX_S: i64 = 11_644_473_600;
    /// FILETIME's 100 ns steps in a second.
    pub const FILETIME_STEPS_PER_S: i64 = 10_000_000;

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
        pub fn SystemTimeToTzSpecificLocalTime(
            zone: *const std::ffi::c_void,
            utc: *const SystemTime,
            local: *mut SystemTime,
        ) -> i32;
    }
}

/// This computer's offset from UTC (local minus UTC, seconds) at a moment (seconds since 1970), daylight saving time
/// included, as Python's local time conversions take it: Windows' time zone, or on Linux and macOS the system's (TZ,
/// else /etc/localtime), read by the C library's localtime_r. 0 where it cannot be read. (The browser build reads the
/// browser's: disk.rs.)
#[cfg(feature = "native")]
pub fn utc_offset_at(secs: f64) -> i64 {
    #[cfg(unix)]
    {
        unsafe extern "C" {
            // POSIX's (the libc crate declares it only for Windows)
            fn tzset();
        }
        let time = secs.floor() as libc::time_t;
        // SAFETY: tm is plain data that localtime_r fills; tzset reads the time zone (nothing here changes TZ)
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        unsafe { tzset() };
        if !unsafe { libc::localtime_r(&time, &mut tm) }.is_null() {
            return tm.tm_gmtoff as i64;
        }
    }
    #[cfg(windows)]
    {
        let file_time = ((secs.floor() as i64 + win::FILETIME_TO_UNIX_S) * win::FILETIME_STEPS_PER_S) as u64;
        let (mut utc, mut local, mut back) = (win::SystemTime::default(), win::SystemTime::default(), 0u64);
        // SAFETY: each call reads and writes the structs it is given
        let ok = unsafe {
            win::FileTimeToSystemTime(&file_time, &mut utc) != 0
                && win::SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut local) != 0
                && win::SystemTimeToFileTime(&local, &mut back) != 0
        };
        if ok {
            return (back as i64 - file_time as i64).div_euclid(win::FILETIME_STEPS_PER_S);
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

/// The measures of a stats file's run from the newest log in `dir` that covers it (see `Library::mouse_measures`).
pub fn measures_in(dir: &Path, stats_path: &Path) -> Answer<Value> {
    let stats_name = file_name(stats_path).unwrap_or_default();
    let text = String::from_utf8_lossy(&read_named(stats_path)?).into_owned();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_are_whole_quarter_hours() {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64();
        assert_eq!(utc_offset_at(now) % 900, 0);
    }

    /// Off Windows the offset follows the system's time zone and its daylight saving time: New York's (as a POSIX TZ,
    /// which needs no time zone files), in a child process of this test, so no other test sees TZ change.
    #[cfg(unix)]
    #[test]
    fn offsets_follow_the_time_zone() {
        if std::env::var_os("AIMVIEW_TZ_CHILD").is_some() {
            assert_eq!(utc_offset_at(1_704_067_200.0), -5 * 3600, "2024-01-01 00:00 UTC: EST");
            assert_eq!(utc_offset_at(1_719_792_000.0), -4 * 3600, "2024-07-01 00:00 UTC: EDT");
            assert_eq!(crate::library::local_stamp(1_719_792_000.0), "2024.06.30-20.00.00");
            return;
        }
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "mouse::tests::offsets_follow_the_time_zone"])
            .env("AIMVIEW_TZ_CHILD", "1")
            .env("TZ", "EST5EDT,M3.2.0,M11.1.0")
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success() && said.contains("1 passed"), "{said}{}", String::from_utf8_lossy(&out.stderr));
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
        // the other log: the header alone, its start time (nanoseconds, bytes 24 to 32) two hours earlier
        let mut other = log[..reader::HEADER_SIZE].to_vec();
        let start_ns = i64::from_le_bytes(log[24..32].try_into().unwrap());
        other[24..32].copy_from_slice(&(start_ns - 7_200_000_000_000).to_le_bytes());
        std::fs::write(dir.join("mouse_2026-09-30_04-54-21.bin"), &other).unwrap();
        let found = measures_in(&dir, &stats);
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
