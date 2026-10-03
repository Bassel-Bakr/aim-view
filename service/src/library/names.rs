//! File names and time stamps: a recording's name as KovOBS writes it, a stats file's as KovaaK writes it, their time
//! stamps, and a recording's folder name (python/server.py: NAME, STATS_NAME, stamp_seconds, cache_dir).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

/// A recording's name as KovOBS writes it: "<scenario> - <score> - <yyyy.mm.dd-hh.mm.ss>.mp4": (scenario, score, stamp).
pub fn parse_name(name: &str) -> Option<(String, f64, String)> {
    let rest = name.strip_suffix(".mp4")?;
    let (rest, stamp) = rest.rsplit_once(" - ")?;
    let (scenario, score) = rest.rsplit_once(" - ")?;
    let numeric = !score.is_empty() && score.chars().all(|c| c.is_ascii_digit() || c == '-' || c == '.');
    if !numeric || scenario.is_empty() || stamp_seconds(stamp).is_none() {
        return None;
    }
    Some((scenario.to_string(), score.parse().ok()?, stamp.to_string()))
}

/// A video's name as KovOBS would write it (any video is read as its .mp4 name).
pub(crate) fn parse_video(video: &Path) -> Option<(String, f64, String)> {
    parse_name(&video.with_extension("mp4").file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
}

/// A stats file's name as KovaaK writes it: "<scenario> - Challenge - <stamp> Stats.csv": (scenario, stamp).
pub fn parse_stats_name(name: &str) -> Option<(String, String)> {
    let rest = name.strip_suffix(" Stats.csv")?;
    let (rest, stamp) = rest.rsplit_once(" - ")?;
    let scenario = rest.strip_suffix(" - Challenge")?;
    stamp_seconds(stamp).map(|_| (scenario.to_string(), stamp.to_string()))
}

/// A file-name time stamp (yyyy.mm.dd-hh.mm.ss, local time) as seconds from 2000-01-01, for differences only. Some
/// KovOBS recordings from June 2026 are named with the year 0026: read as 2026.
pub fn stamp_seconds(stamp: &str) -> Option<f64> {
    let b = stamp.as_bytes();
    if b.len() != 19 || b[4] != b'.' || b[7] != b'.' || b[10] != b'-' || b[13] != b'.' || b[16] != b'.' {
        return None;
    }
    let num = |from: usize, to: usize| stamp.get(from..to)?.parse::<i64>().ok();
    let mut year = num(0, 4)?;
    if stamp.starts_with("00") {
        year += 2000;
    }
    let (month, day, h, m, s) = (num(5, 7)?, num(8, 10)?, num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || h > 23 || m > 59 || s > 60 {
        return None;
    }
    // days from the civil date (Howard Hinnant's algorithm), from 2000-01-01
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 730_425;
    Some((days * 86_400 + h * 3600 + m * 60 + s) as f64)
}

/// A recording's folder name: its file name's stem, with anything but word characters, dots and dashes as "_"
/// (python/server.py: cache_dir).
pub fn slug(id: &str) -> String {
    let stem = Path::new(id).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let mut out = String::new();
    let mut gap = false;
    for c in stem.chars() {
        if c.is_alphanumeric() || matches!(c, '_' | '.' | '-') {
            out.push(c);
            gap = false;
        } else if !gap {
            out.push('_');
            gap = true;
        }
    }
    out
}

/// p, or p with " (2)", " (3)" and so on in its name when that file is there already: nothing is overwritten.
pub(crate) fn free_name(p: PathBuf) -> PathBuf {
    let (stem, ext) = (
        p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
        p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default(),
    );
    let mut out = p.clone();
    let mut n = 2;
    while out.exists() {
        out = p.with_file_name(format!("{stem} ({n}){ext}"));
        n += 1;
    }
    out
}

/// A time (seconds since 1970) as a local file-name stamp, for a video added with a name of its own.
pub fn local_stamp(secs: f64) -> String {
    static OFFSET: OnceLock<i64> = OnceLock::new();
    let offset = *OFFSET.get_or_init(local_offset);
    let t = secs as i64 + offset;
    let (days, rem) = (t.div_euclid(86_400), t.rem_euclid(86_400));
    // the civil date from days since 1970 (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}.{month:02}.{day:02}-{:02}.{:02}.{:02}", rem / 3600, rem / 60 % 60, rem % 60)
}

/// This computer's offset from UTC in seconds (its time zone, now).
fn local_offset() -> i64 {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    #[cfg(windows)]
    {
        #[repr(C)]
        struct SystemTimeParts {
            year: u16,
            month: u16,
            weekday: u16,
            day: u16,
            hour: u16,
            minute: u16,
            second: u16,
            ms: u16,
        }
        unsafe extern "system" {
            fn GetLocalTime(t: *mut SystemTimeParts);
        }
        let mut t = SystemTimeParts { year: 0, month: 0, weekday: 0, day: 0, hour: 0, minute: 0, second: 0, ms: 0 };
        // SAFETY: GetLocalTime fills the struct it is given
        unsafe { GetLocalTime(&mut t) };
        let stamp = format!("{:04}.{:02}.{:02}-{:02}.{:02}.{:02}", t.year, t.month, t.day, t.hour, t.minute, t.second);
        if let Some(local) = stamp_seconds(&stamp) {
            // stamp_seconds counts from 2000-01-01; Unix time from 1970-01-01
            let local_unix = local as i64 + 946_684_800;
            return ((local_unix - now) as f64 / 900.0).round() as i64 * 900;
        }
    }
    let _ = now;
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_as_python_reads_them() {
        assert_eq!(
            parse_name("1wall 2targets xsmall - valorant - 558.46 - 2026.10.01-16.23.04.mp4"),
            Some(("1wall 2targets xsmall - valorant".into(), 558.46, "2026.10.01-16.23.04".into()))
        );
        assert_eq!(parse_name("clip.mp4"), None);
        assert_eq!(
            parse_stats_name("1wall 2targets xsmall - valorant - Challenge - 2026.10.01-16.23.04 Stats.csv"),
            Some(("1wall 2targets xsmall - valorant".into(), "2026.10.01-16.23.04".into()))
        );
        assert_eq!(stamp_seconds("2000.01.01-00.00.10"), Some(10.0));
        assert_eq!(stamp_seconds("2000.03.01-00.00.00"), Some(60.0 * 86_400.0));
        assert_eq!(stamp_seconds("0026.06.01-00.00.00"), stamp_seconds("2026.06.01-00.00.00"));
        assert_eq!(local_stamp(946_684_800.0 - local_offset() as f64), "2000.01.01-00.00.00");
        assert_eq!(slug("a/1wall - x (2).mp4"), "1wall_-_x_2_");
    }
}
