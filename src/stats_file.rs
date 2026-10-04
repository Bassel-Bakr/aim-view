//! KovaaK's stats file (review.py: `load_stats`): its "Key:,value" lines and its kill table.
//!
//! In: the file's text (the service finds a run's file in KovaaK's stats folder: service/src/library/stats.rs). Out:
//! each kill's time and shots for the clicking review (review.rs), and the challenge's start for the tracking review
//! (tracking.rs); the mouse log (mouse.rs) splits its own text into lines with `lines`.

use std::collections::HashMap;

/// The "Key:,value" line that says when the challenge started, as a time of day ("%H:%M:%S.%f").
const CHALLENGE_START: &str = "Challenge Start";
/// The kill table's columns: each kill's time of day, its shots and its hits.
const TIME_COLUMN: usize = 1;
const SHOTS_COLUMN: usize = 5;
const HITS_COLUMN: usize = 6;
/// The largest hour, minute and second "%H:%M:%S" reads (Python's `strptime` takes 61 seconds, for leap seconds).
const MAX_HOUR: i64 = 23;
const MAX_MINUTE: i64 = 59;
const MAX_SECOND: i64 = 61;
/// The digits of "%f", the fraction of a second: a shorter one is padded with zeros on the right.
const FRACTION_DIGITS: usize = 6;
/// The longest hour, minute or second "%H", "%M" and "%S" read.
const MAX_FIELD_DIGITS: usize = 2;
const MINUTES_PER_HOUR: i64 = 60;
const SECONDS_PER_MINUTE: i64 = 60;
const MICROS_PER_SECOND: i64 = 1_000_000;

/// A stats file: its "Key:,value" lines (a later line wins), and the kill table's rows as cells of text.
pub struct StatsFile {
    pub meta: HashMap<String, String>,
    pub rows: Vec<Vec<String>>,
}

/// The kills in a stats file: each one's time in seconds since the challenge started, and the shots it took.
pub struct StatsKills {
    pub times: Vec<f64>,
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

/// A time of day as "%H:%M:%S.%f" reads it, in microseconds since midnight.
fn micros(text: &str) -> Option<i64> {
    let (clock, fraction) = text.split_once('.')?;
    let mut fields = clock.split(':');
    let mut field = |max: i64| -> Option<i64> {
        let digits = fields.next()?;
        let well_formed = !digits.is_empty()
            && digits.len() <= MAX_FIELD_DIGITS
            && digits.bytes().all(|b| b.is_ascii_digit());
        let value: i64 = well_formed.then(|| digits.parse().ok())??;
        (value <= max).then_some(value)
    };
    let (hours, minutes, seconds) = (field(MAX_HOUR)?, field(MAX_MINUTE)?, field(MAX_SECOND)?);
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
    pub fn parse(text: &str) -> StatsFile {
        let lines = lines(text);
        let mut meta = HashMap::new();
        for line in &lines {
            if let Some((key, value)) = line.split_once(":,") {
                meta.insert(key.to_string(), value.to_string());
            }
        }
        // the kill rows are the first table, up to its blank line (later tables can also start with a digit)
        let mut rows = Vec::new();
        for line in lines.iter().skip(1) {
            if line.trim().is_empty() {
                break;
            }
            if line.starts_with(|first: char| first.is_ascii_digit()) {
                rows.push(line.split(',').map(str::to_string).collect());
            }
        }
        StatsFile { meta, rows }
    }

    /// When the challenge started, in microseconds since midnight.
    pub fn start_micros(&self) -> Option<i64> {
        micros(self.meta.get(CHALLENGE_START)?)
    }

    /// The shots each kill took (the table's sixth column).
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

    /// Each kill's time since the challenge started, and its shots (review.py: `match`).
    pub fn kills(&self) -> Result<StatsKills, String> {
        let start = self.meta.get(CHALLENGE_START).ok_or("no Challenge Start in the stats file")?;
        let start_micros = micros(start).ok_or_else(|| format!("Challenge Start is not a time: {start}"))?;
        let times = self
            .rows
            .iter()
            .map(|row| {
                let kill_micros = row
                    .get(TIME_COLUMN)
                    .and_then(|cell| micros(cell))
                    .ok_or_else(|| format!("a kill row without a time: {row:?}"))?;
                Ok((kill_micros - start_micros) as f64 / MICROS_PER_SECOND as f64)
            })
            .collect::<Result<_, String>>()?;
        Ok(StatsKills { times, shots: self.shots()? })
    }
}

/// A kill row's whole number in a column, when the row has one there.
fn number(row: &[String], column: usize) -> Option<i64> {
    row.get(column).and_then(|cell| cell.trim().parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
