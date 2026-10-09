//! File names and time stamps: a recording's name as KovOBS writes it, a stats file's as KovaaK writes it, their time
//! stamps, and a recording's folder name (python/retired/server.py: NAME, STATS_NAME, stamp_seconds, cache_dir). In:
//! file names and times. Out: their parts and stamps, for the recordings list, the stats files' pairing, uploads and
//! links.

use std::path::{Path, PathBuf};

use aimview::dates::{civil_from_days, days_from_civil};

/// A file-name time stamp's form (yyyy.mm.dd-hh.mm.ss): its separators in their places; each 0 is a number's digit.
const STAMP_FORM: &[u8; 19] = b"0000.00.00-00.00.00";
/// Added to a year written as 00yy: some KovOBS recordings from June 2026 are named with the year 0026.
const MISSING_CENTURIES: i64 = 2000;
/// The days from 1970-01-01, where the core's civil dates count from (src/dates.rs), to 2000-01-01, where a stamp's
/// seconds count from.
const DAYS_1970_TO_2000: i64 = 10_957;
/// The seconds in a day.
const SECONDS_PER_DAY: i64 = 86_400;
/// The seconds in an hour.
const SECONDS_PER_HOUR: i64 = 3600;
/// The seconds in a minute.
const SECONDS_PER_MINUTE: i64 = 60;
/// The minutes in an hour.
const MINUTES_PER_HOUR: i64 = 60;

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
    let days = days_from_civil(year, month, day) - DAYS_1970_TO_2000;
    Some((days * SECONDS_PER_DAY + hour * SECONDS_PER_HOUR + minute * SECONDS_PER_MINUTE + second) as f64)
}

/// A recording's folder name: its file name's stem, with anything but word characters, dots and dashes as "_" (a run
/// of them as one) (python/retired/server.py: cache_dir).
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

/// This computer's offset from UTC in seconds at `secs` (seconds since 1970), daylight saving time included
/// (disk.rs: `utc_offset_at`), so a stamp in summer and one in winter each get their own.
fn offset_at(secs: f64) -> i64 {
    crate::disk::utc_offset_at(secs)
}

/// The names and stamps.
#[cfg(test)]
mod tests {
    use super::*;

    /// A local stamp takes the offset at its own time, not today's: a winter and a summer time (2024-01-01 and
    /// 2024-07-01, 00:00 UTC) each read back as the time plus the offset then. Where the time zone keeps daylight
    /// saving time, the two offsets differ, so one of them is not today's.
    #[test]
    fn stamps_take_the_offset_at_their_time() {
        /// Seconds from 1970-01-01 to 2000-01-01, where `stamp_seconds` counts from.
        const UNIX_TO_2000_S: f64 = 946_684_800.0;
        for secs in [1_704_067_200.0, 1_719_792_000.0] {
            let local_s = stamp_seconds(&local_stamp(secs)).unwrap() + UNIX_TO_2000_S;
            assert_eq!(local_s - secs, crate::disk::utc_offset_at(secs) as f64, "at {secs}");
        }
    }

    /// Recordings', stats files' and links' names, stamps (the year 0026 too) and slugs read as Python read them.
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
