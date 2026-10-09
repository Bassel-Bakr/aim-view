//! KovaaK's stats file (review.py: `load_stats`): its "Key:,value" lines and its kill table.
//!
//! In: the file's text (the service finds a run's file in KovaaK's stats folder: service/src/library/stats.rs). Out:
//! each kill's time and shots for the clicking review (review.rs), and the challenge's start for the tracking review
//! (tracking.rs); the mouse log's reader (mouse.rs) reads its own way from the same lines, "Key:,value" lines, kill
//! table, columns and times of day.

use std::collections::HashMap;

/// The "Key:,value" line that says when the challenge started, as a time of day ("%H:%M:%S.%f").
pub(crate) const CHALLENGE_START: &str = "Challenge Start";
/// The kill table's column of each kill's time of day (from 0: the second column, "Timestamp").
pub(crate) const TIME_COLUMN: usize = 1;
/// The kill table's column of each kill's shots (the sixth).
pub(crate) const SHOTS_COLUMN: usize = 5;
/// The kill table's column of each kill's hits (the seventh).
const HITS_COLUMN: usize = 6;
/// The largest hour "%H" reads.
const MAX_HOUR: i64 = 23;
/// The largest minute "%M" reads.
const MAX_MINUTE: i64 = 59;
/// The largest second "%S" reads: Python's `strptime` takes 61, for leap seconds.
const MAX_SECOND: i64 = 61;
/// The digits of "%f", the fraction of a second: a shorter one is padded with zeros on the right.
const FRACTION_DIGITS: usize = 6;
/// The longest hour, minute or second "%H", "%M" and "%S" read.
const MAX_FIELD_DIGITS: usize = 2;
/// Minutes in an hour, for the time of day in microseconds.
const MINUTES_PER_HOUR: i64 = 60;
/// Seconds in a minute, for the time of day in microseconds.
const SECONDS_PER_MINUTE: i64 = 60;
/// Microseconds in a second: the times' unit before they become seconds.
const MICROS_PER_SECOND: i64 = 1_000_000;
/// Microseconds in a day: a kill's time of day before the start's is on the next day (a run across midnight).
const MICROS_PER_DAY: i64 = 86_400 * MICROS_PER_SECOND;

/// A stats file: its "Key:,value" lines (a later line wins), and the kill table's rows as cells of text.
pub struct StatsFile {
    /// Each "Key:,value" line's value by its key (without the colon), such as "Challenge Start" or "Kills".
    pub meta: HashMap<String, String>,
    /// The kill table's rows, one a kill in the file's order, each split at its commas.
    pub rows: Vec<Vec<String>>,
}

/// The kills in a stats file: each one's time in seconds since the challenge started, and the shots it took.
pub struct StatsKills {
    /// Each kill's time in seconds since the challenge started, in the file's order.
    pub times: Vec<f64>,
    /// Each kill's shots, in the same order.
    pub shots: Vec<i64>,
}

/// The text's lines, split where Python's `str.splitlines` splits them.
pub(crate) fn lines(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((i, character)) = chars.next() {
        let end = match character {
            '\r' if chars.peek().is_some_and(|&(_, next)| next == '\n') => {
                chars.next();
                i + 2
            }
            '\n' | '\r' | '\x0b' | '\x0c' | '\x1c' | '\x1d' | '\x1e' | '\u{85}' | '\u{2028}' | '\u{2029}' => {
                i + character.len_utf8()
            }
            _ => continue,
        };
        out.push(&text[start..i]);
        start = end;
    }
    if start < text.len() {
        out.push(&text[start..]);
    }
    out
}

/// The "Key:,value" lines' values by their keys (a later line wins).
pub(crate) fn key_values<'a>(lines: &[&'a str]) -> HashMap<&'a str, &'a str> {
    lines.iter().filter_map(|line| line.split_once(":,")).collect()
}

/// The kill table's lines: those after the file's header line, up to the first blank line (later tables follow it).
pub(crate) fn table_lines<'a>(lines: &[&'a str]) -> impl Iterator<Item = &'a str> {
    lines.iter().skip(1).take_while(|line| !line.trim().is_empty()).copied()
}

/// A time of day as "%H:%M:%S.%f" reads it, in microseconds since midnight, with its second at most `max_second`
/// (MAX_SECOND for `strptime`; Python's `datetime` takes up to 59); None when the text is not one.
pub(crate) fn time_of_day_micros(text: &str, max_second: i64) -> Option<i64> {
    let (clock, fraction) = text.split_once('.')?;
    let mut fields = clock.split(':');
    let mut field = |max: i64| -> Option<i64> {
        let digits = fields.next()?;
        let well_formed =
            !digits.is_empty() && digits.len() <= MAX_FIELD_DIGITS && digits.bytes().all(|b| b.is_ascii_digit());
        let value: i64 = well_formed.then(|| digits.parse().ok())??;
        (value <= max).then_some(value)
    };
    let (hours, minutes, seconds) = (field(MAX_HOUR)?, field(MAX_MINUTE)?, field(max_second)?);
    if fields.next().is_some()
        || fraction.is_empty()
        || fraction.len() > FRACTION_DIGITS
        || !fraction.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let fraction_micros: i64 = format!("{fraction:0<FRACTION_DIGITS$}").parse().ok()?;
    Some(((hours * MINUTES_PER_HOUR + minutes) * SECONDS_PER_MINUTE + seconds) * MICROS_PER_SECOND + fraction_micros)
}

impl StatsFile {
    /// Reads a stats file's text: every "Key:,value" line, and the kill table's rows (the lines after the header that
    /// start with a digit, up to the first blank line). Never fails: a file with no table gives no rows.
    pub fn parse(text: &str) -> StatsFile {
        let lines = lines(text);
        let meta = key_values(&lines).into_iter().map(|(key, value)| (key.to_string(), value.to_string())).collect();
        // the kill rows are the first table, up to its blank line (later tables can also start with a digit)
        let rows = table_lines(&lines)
            .filter(|line| line.starts_with(|first: char| first.is_ascii_digit()))
            .map(|line| line.split(',').map(str::to_string).collect())
            .collect();
        StatsFile { meta, rows }
    }

    /// When the challenge started, in microseconds since midnight; None when the file lacks the line or its time.
    pub fn start_micros(&self) -> Option<i64> {
        time_of_day_micros(self.meta.get(CHALLENGE_START)?, MAX_SECOND)
    }

    /// The shots each kill took (the table's sixth column); an error names the first row without them.
    pub fn shots(&self) -> Result<Vec<i64>, String> {
        self.rows
            .iter()
            .map(|row| number(row, SHOTS_COLUMN).ok_or_else(|| format!("a kill row without shots: {row:?}")))
            .collect()
    }

    /// The hits each kill took (the table's seventh column), or None when a row lacks them.
    pub fn hits(&self) -> Option<Vec<i64>> {
        self.rows.iter().map(|row| number(row, HITS_COLUMN)).collect()
    }

    /// Each kill's time since the challenge started, and its shots (review.py: `match`; a run across midnight counts
    /// on, where review.py gave negative times). An error when the file has no Challenge Start time or a row lacks its
    /// time or shots.
    pub fn kills(&self) -> Result<StatsKills, String> {
        let start = self.meta.get(CHALLENGE_START).ok_or("no Challenge Start in the stats file")?;
        let start_micros =
            time_of_day_micros(start, MAX_SECOND).ok_or_else(|| format!("Challenge Start is not a time: {start}"))?;
        let times = self
            .rows
            .iter()
            .map(|row| {
                let kill_micros = row
                    .get(TIME_COLUMN)
                    .and_then(|cell| time_of_day_micros(cell, MAX_SECOND))
                    .ok_or_else(|| format!("a kill row without a time: {row:?}"))?;
                // a run is far shorter than a day: a kill before the start in the day is after midnight
                Ok((kill_micros - start_micros).rem_euclid(MICROS_PER_DAY) as f64 / MICROS_PER_SECOND as f64)
            })
            .collect::<Result<_, String>>()?;
        Ok(StatsKills { times, shots: self.shots()? })
    }
}

/// A kill row's whole number in a column, when the row has one there.
fn number(row: &[String], column: usize) -> Option<i64> {
    row.get(column).and_then(|cell| cell.trim().parse().ok())
}

/// Checks reading a short stats file.
#[cfg(test)]
mod tests {
    use super::*;

    /// The kill rows stop at the blank line, the "Key:,value" lines are read, and the kills' times count from the
    /// challenge's start.
    #[test]
    fn reads_the_kill_table() {
        let text = "Kill #,Timestamp,Bot,Weapon,TTK,Shots,Hits\r\n1,16:20:01.5,b,w,0.1s,1,1\r\n\
                    2,16:20:02.25,b,w,0.1s,2,1\r\n\r\n16ML9 - 080,1\r\nKills:,2\r\nChallenge Start:,16:20:00.000\r\n";
        let file = StatsFile::parse(text);
        assert_eq!(file.rows.len(), 2);
        assert_eq!(file.meta["Kills"], "2");
        let kills = file.kills().unwrap();
        assert_eq!(kills.times, vec![1.5, 2.25]);
        assert_eq!(kills.shots, vec![1, 2]);
        assert_eq!(file.hits(), Some(vec![1, 1]));
    }

    /// A run that started before midnight counts its kills after it on: 23:59:59 to 00:00:01 is 2 s, not minus a day.
    #[test]
    fn a_run_across_midnight_counts_on() {
        let text = "Kill #,Timestamp,Bot,Weapon,TTK,Shots,Hits\n1,23:59:59.5,b,w,0.1s,1,1\n\
                    2,00:00:01.0,b,w,0.1s,1,1\n\nChallenge Start:,23:59:59.000\n";
        assert_eq!(StatsFile::parse(text).kills().unwrap().times, vec![0.5, 2.0]);
    }
}
