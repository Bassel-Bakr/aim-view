//! KovaaK's stats file (review.py: `load_stats`): its "Key:,value" lines and its kill table.

use std::collections::HashMap;

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
    while let Some((i, c)) = chars.next() {
        let end = match c {
            '\r' if chars.peek().is_some_and(|&(_, n)| n == '\n') => {
                chars.next();
                i + 2
            }
            '\n' | '\r' | '\x0b' | '\x0c' | '\x1c' | '\x1d' | '\x1e' | '\u{85}' | '\u{2028}' | '\u{2029}' => {
                i + c.len_utf8()
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

/// A time of day as "%H:%M:%S.%f" reads it, in microseconds.
fn micros(text: &str) -> Option<i64> {
    let (hms, frac) = text.split_once('.')?;
    let mut parts = hms.split(':');
    let mut field = |max: i64| -> Option<i64> {
        let s = parts.next()?;
        let v: i64 = (!s.is_empty() && s.len() <= 2 && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.parse().ok())??;
        (v <= max).then_some(v)
    };
    let (h, m, s) = (field(23)?, field(59)?, field(61)?);
    if parts.next().is_some() || frac.is_empty() || frac.len() > 6 || !frac.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let f: i64 = format!("{frac:0<6}").parse().ok()?;
    Some(((h * 60 + m) * 60 + s) * 1_000_000 + f)
}

impl StatsFile {
    pub fn parse(text: &str) -> StatsFile {
        let lines = lines(text);
        let mut meta = HashMap::new();
        for l in &lines {
            if let Some((k, v)) = l.split_once(":,") {
                meta.insert(k.to_string(), v.to_string());
            }
        }
        // the kill rows are the first table, up to its blank line (later tables can also start with a digit)
        let mut rows = Vec::new();
        for l in lines.iter().skip(1) {
            if l.trim().is_empty() {
                break;
            }
            if l.starts_with(|c: char| c.is_ascii_digit()) {
                rows.push(l.split(',').map(str::to_string).collect());
            }
        }
        StatsFile { meta, rows }
    }

    /// When the challenge started, in microseconds since midnight.
    pub fn start_micros(&self) -> Option<i64> {
        micros(self.meta.get("Challenge Start")?)
    }

    /// The shots each kill took (the table's sixth column).
    pub fn shots(&self) -> Result<Vec<i64>, String> {
        self.rows
            .iter()
            .map(|r| r.get(5).and_then(|s| s.trim().parse().ok()).ok_or_else(|| format!("a kill row without shots: {r:?}")))
            .collect()
    }

    /// The hits each kill took (the table's seventh column), or None when a row lacks them.
    pub fn hits(&self) -> Option<Vec<i64>> {
        self.rows.iter().map(|r| r.get(6).and_then(|s| s.trim().parse().ok())).collect()
    }

    /// Each kill's time since the challenge started, and its shots (review.py: `match`).
    pub fn kills(&self) -> Result<StatsKills, String> {
        let start = self.meta.get("Challenge Start").ok_or("no Challenge Start in the stats file")?;
        let t0 = micros(start).ok_or_else(|| format!("Challenge Start is not a time: {start}"))?;
        let times = self
            .rows
            .iter()
            .map(|r| {
                let t = r.get(1).and_then(|s| micros(s)).ok_or_else(|| format!("a kill row without a time: {r:?}"))?;
                Ok((t - t0) as f64 / 1e6)
            })
            .collect::<Result<_, String>>()?;
        Ok(StatsKills { times, shots: self.shots()? })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_kill_table() {
        let text = "Kill #,Timestamp,Bot,Weapon,TTK,Shots,Hits\r\n1,16:20:01.5,b,w,0.1s,1,1\r\n2,16:20:02.25,b,w,0.1s,2,1\r\n\r\n16ML9 - 080,1\r\nKills:,2\r\nChallenge Start:,16:20:00.000\r\n";
        let s = StatsFile::parse(text);
        assert_eq!(s.rows.len(), 2);
        assert_eq!(s.meta["Kills"], "2");
        let k = s.kills().unwrap();
        assert_eq!(k.times, vec![1.5, 2.25]);
        assert_eq!(k.shots, vec![1, 2]);
        assert_eq!(s.hits(), Some(vec![1, 1]));
    }
}
