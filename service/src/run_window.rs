//! The user's run window for a recording: where the run starts and ends, kept as run.json in its review folder (as
//! python/server.py keeps it). The report measures only that part, and a review tracks only it, with a margin.

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::review::TimeWindow;

/// Seconds tracked either side of the window, so its first and last kills are whole (as the browser does).
const MARGIN: f64 = 1.0;
/// The latest a mark can be, in seconds (python/server.py: set_run).
const LONGEST: f64 = 36000.0;
const FILE: &str = "run.json";

/// The marks in seconds, any of them None.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct RunMarks {
    pub start: Option<f64>,
    pub end: Option<f64>,
    pub length: Option<f64>,
}

impl RunMarks {
    /// The recording's marks; none when it has no run.json.
    pub fn read(dir: &Path) -> RunMarks {
        std::fs::read(dir.join(FILE)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn is_set(&self) -> bool {
        self.start.is_some() || self.end.is_some() || self.length.is_some()
    }

    /// Marks sent by the page ({start, end, length}, each a number or null), checked as python/server.py checks them.
    pub fn parse(body: &Value) -> Result<RunMarks, String> {
        let mark = |k: &str| -> Result<Option<f64>, String> {
            match &body[k] {
                Value::Null => Ok(None),
                Value::String(s) if s.is_empty() => Ok(None),
                v => match v.as_f64() {
                    Some(t) if (0.0..LONGEST).contains(&t) => Ok(Some(t)),
                    _ => Err(format!("{k} out of range")),
                },
            }
        };
        let marks = RunMarks { start: mark("start")?, end: mark("end")?, length: mark("length")? };
        if let (Some(a), Some(b)) = (marks.start, marks.end)
            && b <= a
        {
            return Err("the end must come after the start".into());
        }
        Ok(marks)
    }

    /// Keeps the marks; none set forgets them.
    pub fn save(&self, dir: &Path) -> Result<(), String> {
        let path = dir.join(FILE);
        if !self.is_set() {
            let _ = std::fs::remove_file(&path);
            return Ok(());
        }
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        std::fs::write(path, serde_json::to_vec(self).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
    }

    /// The part of the video to track (python/review.py's run_window, in seconds, with a margin): start and end, or one
    /// of them and the length (else the scenario's `limit`); None for the whole video.
    pub fn tracked(&self, limit: Option<f64>) -> Option<TimeWindow> {
        let length = self.length.filter(|&l| l != 0.0).or(limit.filter(|&l| l != 0.0));
        let (start, end) = match (self.start, self.end, length) {
            (Some(a), Some(b), _) if b > a => (a, b),
            (Some(a), _, l) => (a, l.map_or(f64::INFINITY, |l| a + l)),
            (None, Some(b), Some(l)) => ((b - l).max(0.0), b),
            _ => return None,
        };
        Some(TimeWindow { start: (start - MARGIN).max(0.0), end: end + MARGIN })
    }
}

/// Whether tracks made over `tracked` (None: the whole video) hold all of `wanted` (None: the whole video).
pub fn covers(tracked: Option<TimeWindow>, wanted: Option<TimeWindow>) -> bool {
    match (tracked, wanted) {
        (None, _) => true,
        (Some(t), Some(w)) => t.start <= w.start && t.end >= w.end,
        (Some(_), None) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn marks_as_the_server_and_the_browser_read_them() {
        let m = RunMarks::parse(&json!({"start": 2.0, "end": 30.0, "length": null})).unwrap();
        assert_eq!(m.tracked(Some(60.0)), Some(TimeWindow { start: 1.0, end: 31.0 }));
        let start = RunMarks { start: Some(5.0), ..Default::default() };
        assert_eq!(start.tracked(Some(60.0)), Some(TimeWindow { start: 4.0, end: 66.0 }));
        let end = RunMarks { end: Some(10.0), length: Some(20.0), ..Default::default() };
        assert_eq!(end.tracked(None), Some(TimeWindow { start: 0.0, end: 11.0 }));
        assert_eq!(RunMarks::default().tracked(Some(60.0)), None);
        assert!(RunMarks::parse(&json!({"start": 9.0, "end": 3.0, "length": null})).is_err());
        assert!(RunMarks::parse(&json!({"start": -1.0, "end": null, "length": null})).is_err());
        let wide = Some(TimeWindow { start: 1.0, end: 31.0 });
        assert!(covers(None, wide) && covers(wide, Some(TimeWindow { start: 2.0, end: 30.0 })));
        assert!(!covers(wide, None) && !covers(wide, Some(TimeWindow { start: 0.0, end: 30.0 })));
    }
}
