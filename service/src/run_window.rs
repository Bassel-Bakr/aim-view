//! The user's run window for a recording: where the run starts and ends, kept with the recording (store.rs; as
//! python/server.py keeps it, run.json in its folder). In: the marks the page sends (/api/run). Out: the kept marks,
//! the part of the video a review tracks (with a margin, library/reviews.rs) and the marks the report measures within
//! (report.rs).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::review::TimeWindow;
use crate::store::{Item, Mark, Store};

/// Seconds tracked either side of the window, so its first and last kills are whole (as the browser does).
const MARGIN_S: f64 = 1.0;
/// The latest a mark can be, in seconds (python/server.py: set_run).
const LONGEST_S: f64 = 36000.0;

/// The marks in seconds, any of them None.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct RunMarks {
    pub start: Option<f64>,
    pub end: Option<f64>,
    pub length: Option<f64>,
}

impl RunMarks {
    /// The recording's marks; none when none are kept.
    pub fn read(store: &dyn Store, id: &str) -> RunMarks {
        let bytes = store.read(Item::Mark(id, Mark::RunWindow)).ok().flatten();
        bytes.and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
    }

    pub fn is_set(&self) -> bool {
        self.start.is_some() || self.end.is_some() || self.length.is_some()
    }

    /// Marks sent by the page ({start, end, length}, each a number or null), checked as python/server.py checks them.
    pub fn parse(body: &Value) -> Result<RunMarks, String> {
        let mark = |key: &str| -> Result<Option<f64>, String> {
            match &body[key] {
                Value::Null => Ok(None),
                Value::String(text) if text.is_empty() => Ok(None),
                value => match value.as_f64() {
                    Some(seconds) if (0.0..LONGEST_S).contains(&seconds) => Ok(Some(seconds)),
                    _ => Err(format!("{key} out of range")),
                },
            }
        };
        let marks = RunMarks { start: mark("start")?, end: mark("end")?, length: mark("length")? };
        if let (Some(start), Some(end)) = (marks.start, marks.end)
            && end <= start
        {
            return Err("the end must come after the start".into());
        }
        Ok(marks)
    }

    /// Keeps the recording's marks; none set forgets them.
    pub fn save(&self, store: &dyn Store, id: &str) -> Result<(), String> {
        let item = Item::Mark(id, Mark::RunWindow);
        if !self.is_set() {
            let _ = store.remove(item);
            return Ok(());
        }
        let bytes = serde_json::to_vec(self).map_err(|error| error.to_string())?;
        store.write(item, &bytes).map_err(|error| error.to_string())
    }

    /// The part of the video to track (python/retired/review.py's run_window, in seconds, with a margin): start and
    /// end, or one of them and the length (else the scenario's `limit`); None for the whole video.
    pub fn tracked(&self, limit: Option<f64>) -> Option<TimeWindow> {
        let length = self.length.filter(|&seconds| seconds != 0.0).or(limit.filter(|&seconds| seconds != 0.0));
        let (start, end) = match (self.start, self.end, length) {
            (Some(start), Some(end), _) if end > start => (start, end),
            (Some(start), _, length) => (start, length.map_or(f64::INFINITY, |length| start + length)),
            (None, Some(end), Some(length)) => ((end - length).max(0.0), end),
            _ => return None,
        };
        Some(TimeWindow { start: (start - MARGIN_S).max(0.0), end: end + MARGIN_S })
    }
}

/// Whether tracks made over `tracked` (None: the whole video) hold all of `wanted` (None: the whole video).
pub fn covers(tracked: Option<TimeWindow>, wanted: Option<TimeWindow>) -> bool {
    match (tracked, wanted) {
        (None, _) => true,
        (Some(tracked), Some(wanted)) => tracked.start <= wanted.start && tracked.end >= wanted.end,
        (Some(_), None) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn marks_as_the_server_and_the_browser_read_them() {
        let marks = RunMarks::parse(&json!({"start": 2.0, "end": 30.0, "length": null})).unwrap();
        assert_eq!(marks.tracked(Some(60.0)), Some(TimeWindow { start: 1.0, end: 31.0 }));
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
