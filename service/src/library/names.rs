//! File names and time stamps: a recording's name as KovOBS writes it, a stats file's as KovaaK writes it, their time
//! stamps, and a recording's folder name (python/server.py: NAME, STATS_NAME, stamp_seconds, cache_dir). In: file names
//! and times. Out: their parts and stamps, for the recordings list, the stats files' pairing, uploads and links.

use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::sync::OnceLock;
#[cfg(windows)]
use std::time::{SystemTime, UNIX_EPOCH};

/// A file-name time stamp's form (yyyy.mm.dd-hh.mm.ss): its separators in their places; each 0 is a number's digit.
const STAMP_FORM: &[u8; 19] = b"0000.00.00-00.00.00";
/// Added to a year written as 00yy: some KovOBS recordings from June 2026 are named with the year 0026.
const MISSING_CENTURIES: i64 = 2000;
/// Howard Hinnant's civil calendar: years in an era (the Gregorian cycle), the days in one, and the days from
/// 0000-03-01 to 1970-01-01 and to 2000-01-01.
const YEARS_PER_ERA: i64 = 400;
const DAYS_PER_ERA: i64 = 146_097;
const DAYS_TO_UNIX_EPOCH: i64 = 719_468;
const DAYS_TO_2000: i64 = 730_425;
const SECONDS_PER_DAY: i64 = 86_400;
const SECONDS_PER_HOUR: i64 = 3600;
const SECONDS_PER_MINUTE: i64 = 60;
const MINUTES_PER_HOUR: i64 = 60;
/// Seconds from 1970-01-01 to 2000-01-01.
#[cfg(windows)]
const UNIX_TO_2000_S: i64 = 946_684_800;
/// Time zones are whole quarter hours from UTC: the measured offset is rounded to one, in seconds.
#[cfg(windows)]
const QUARTER_HOUR_S: i64 = 900;

/// A recording's name as KovOBS writes it: "<scenario> - <score> - <yyyy.mm.dd-hh.mm.ss>.mp4": (scenario, score,
/// stamp).
pub fn parse_name(name: &str) -> Option<(String, f64, String)> {
    let rest = name.strip_suffix(".mp4")?;
    let (rest, stamp) = rest.rsplit_once(" - ")?;
    let (scenario, score) = rest.rsplit_once(" - ")?;
    let in_number = |character: char| character.is_ascii_digit() || matches!(character, '-' | '.');
    let numeric = !score.is_empty() && score.chars().all(in_number);
    if !numeric || scenario.is_empty() || stamp_seconds(stamp).is_none() {
        return None;
    }
    Some((scenario.to_string(), score.parse().ok()?, stamp.to_string()))
}

/// A video's name as KovOBS would write it (any video is read as its .mp4 name).
pub(crate) fn parse_video(video: &Path) -> Option<(String, f64, String)> {
    let name = video.with_extension("mp4").file_name().map(|name| name.to_string_lossy().into_owned());
    parse_name(&name.unwrap_or_default())
}

/// A video's name of a title and a time stamp, as a recording added from a link is named: "<title> - <stamp>.mp4":
/// (title, stamp).
pub(crate) fn parse_titled(video: &Path) -> Option<(String, String)> {
    let stem = video.file_stem()?.to_string_lossy().into_owned();
    let (title, stamp) = stem.rsplit_once(" - ")?;
    (!title.is_empty() && stamp_seconds(stamp).is_some()).then(|| (title.to_string(), stamp.to_string()))
}

/// A stats file's name as KovaaK writes it: "<scenario> - Challenge - <stamp> Stats.csv": (scenario, stamp).
pub fn parse_stats_name(name: &str) -> Option<(String, String)> {
    let rest = name.strip_suffix(" Stats.csv")?;
    let (rest, stamp) = rest.rsplit_once(" - ")?;
    let scenario = rest.strip_suffix(" - Challenge")?;
    stamp_seconds(stamp).map(|_| (scenario.to_string(), stamp.to_string()))
}

/// Days from 2000-01-01 to a civil date (Howard Hinnant's `days_from_civil`, counting years from March so that a
/// leap day ends the year).
fn days_since_2000(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(YEARS_PER_ERA);
    let year_of_era = year - era * YEARS_PER_ERA;
    // the days before the month, counted from March: its lengths follow (153 * month + 2) / 5
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * DAYS_PER_ERA + day_of_era - DAYS_TO_2000
}

/// The civil date (year, month, day) of a day counted from 1970-01-01 (Howard Hinnant's `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let from_march_0000 = days + DAYS_TO_UNIX_EPOCH;
    let era = from_march_0000.div_euclid(DAYS_PER_ERA);
    let day_of_era = from_march_0000 - era * DAYS_PER_ERA;
    // the leap days so far in the era taken out, so 365 days make each year
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_from_march = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_from_march + 2) / 5 + 1;
    let month = if month_from_march < 10 { month_from_march + 3 } else { month_from_march - 9 };
    (year_of_era + era * YEARS_PER_ERA + i64::from(month <= 2), month, day)
}

/// A file-name time stamp (yyyy.mm.dd-hh.mm.ss, local time) as seconds from 2000-01-01, for differences only. Some
/// KovOBS recordings from June 2026 are named with the year 0026: read as 2026.
pub fn stamp_seconds(stamp: &str) -> Option<f64> {
    let bytes = stamp.as_bytes();
    let separated = STAMP_FORM.iter().zip(bytes).all(|(&form, &byte)| form == b'0' || byte == form);
    if bytes.len() != STAMP_FORM.len() || !separated {
        return None;
    }
    // the numbers between STAMP_FORM's separators, by their bytes
    let number_at = |from: usize, to: usize| stamp.get(from..to)?.parse::<i64>().ok();
    let mut year = number_at(0, 4)?;
    if stamp.starts_with("00") {
        year += MISSING_CENTURIES;
    }
    let (month, day) = (number_at(5, 7)?, number_at(8, 10)?);
    let (hour, minute, second) = (number_at(11, 13)?, number_at(14, 16)?, number_at(17, 19)?);
    // a second of 60 is a leap second
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let days = days_since_2000(year, month, day);
    Some((days * SECONDS_PER_DAY + hour * SECONDS_PER_HOUR + minute * SECONDS_PER_MINUTE + second) as f64)
}

/// A recording's folder name: its file name's stem, with anything but word characters, dots and dashes as "_"
/// (python/server.py: cache_dir).
pub fn slug(id: &str) -> String {
    let stem = Path::new(id).file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
    let mut out = String::new();
    let mut gap = false;
    for character in stem.chars() {
        if character.is_alphanumeric() || matches!(character, '_' | '.' | '-') {
            out.push(character);
            gap = false;
        } else if !gap {
            out.push('_');
            gap = true;
        }
    }
    out
}

/// `path`, or `path` with " (2)", " (3)" and so on in its name when that file is there already: nothing is
/// overwritten.
pub(crate) fn free_name(path: PathBuf) -> PathBuf {
    let stem = path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
    let extension = path.extension().map(|extension| format!(".{}", extension.to_string_lossy())).unwrap_or_default();
    let mut out = path.clone();
    let mut copy = 2;
    while crate::disk::exists(&out) {
        out = path.with_file_name(format!("{stem} ({copy}){extension}"));
        copy += 1;
    }
    out
}

/// A time (seconds since 1970) as a local file-name stamp, for a video added with a name of its own.
pub fn local_stamp(secs: f64) -> String {
    let local_s = secs as i64 + offset_at(secs);
    let (days, second_of_day) = (local_s.div_euclid(SECONDS_PER_DAY), local_s.rem_euclid(SECONDS_PER_DAY));
    let (year, month, day) = civil_from_days(days);
    let (hour, minute, second) = (
        second_of_day / SECONDS_PER_HOUR,
        second_of_day / SECONDS_PER_MINUTE % MINUTES_PER_HOUR,
        second_of_day % SECONDS_PER_MINUTE,
    );
    format!("{year:04}.{month:02}.{day:02}-{hour:02}.{minute:02}.{second:02}")
}

/// This computer's offset from UTC in seconds for a local stamp: on Windows its time zone's now (read once), elsewhere
/// the system's time zone's at that time, daylight saving time included (mouse.rs: `utc_offset_at`).
#[cfg(windows)]
fn offset_at(_secs: f64) -> i64 {
    static OFFSET: OnceLock<i64> = OnceLock::new();
    *OFFSET.get_or_init(local_offset)
}

#[cfg(not(windows))]
fn offset_at(secs: f64) -> i64 {
    crate::disk::utc_offset_at(secs)
}

/// This computer's offset from UTC in seconds (its time zone, now): its local time now, read as a stamp, less the
/// time now, rounded to a quarter hour.
#[cfg(windows)]
fn local_offset() -> i64 {
    /// SYSTEMTIME.
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
        fn GetLocalTime(parts: *mut SystemTimeParts);
    }
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs() as i64);
    let mut parts = SystemTimeParts { year: 0, month: 0, weekday: 0, day: 0, hour: 0, minute: 0, second: 0, ms: 0 };
    // SAFETY: GetLocalTime fills the struct it is given
    unsafe { GetLocalTime(&mut parts) };
    let SystemTimeParts { year, month, day, hour, minute, second, .. } = parts;
    let stamp = format!("{year:04}.{month:02}.{day:02}-{hour:02}.{minute:02}.{second:02}");
    match stamp_seconds(&stamp) {
        Some(local) => {
            // stamp_seconds counts from 2000-01-01; Unix time from 1970-01-01
            let local_unix = local as i64 + UNIX_TO_2000_S;
            ((local_unix - now) as f64 / QUARTER_HOUR_S as f64).round() as i64 * QUARTER_HOUR_S
        }
        None => 0,
    }
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
        assert_eq!(local_stamp(946_684_800.0 - offset_at(946_684_800.0) as f64), "2000.01.01-00.00.00");
        assert_eq!(slug("a/1wall - x (2).mp4"), "1wall_-_x_2_");
        assert_eq!(
            parse_titled(Path::new("Air 1wall - a run - 2026.10.01-00.00.00.mp4")),
            Some(("Air 1wall - a run".into(), "2026.10.01-00.00.00".into()))
        );
        assert_eq!(parse_titled(Path::new("clip - 2026.mp4")), None);
    }
}
