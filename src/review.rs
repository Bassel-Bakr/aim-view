//! The review of a run with its stats file (review.py: `review`): a clicking run's flicks, or a tracking run's time
//! on the target.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::geometry::{CX, CY, H, K, W};
use crate::matching::{appearances, crosshair_spots, match_times, Flick, KillSource, PathPoint};
use crate::measure::{choices, measure, target_radius, Measure};
use crate::stats_file::StatsFile;
use crate::summary::{judge, summarize, Issue, Mode, Summary};
use crate::track::Tracks;
use crate::tracking::{CameraReading, TrackSummary, countdown_end, stats_length, track_summary};

/// The frame's size and the crosshair's place (pixels), and the focal length (pixels) the degrees come from.
#[derive(Clone, Debug, Serialize)]
#[allow(non_snake_case)]
pub struct Geometry {
    pub W: usize,
    pub H: usize,
    pub CX: f64,
    pub CY: f64,
    pub K: f64,
}

/// A clicking run's report, as report.json keeps it.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub video: String,
    pub stats: Option<String>,
    pub summary: Summary,
    pub issues: Vec<Issue>,
    pub flicks: Vec<Measure>,
    pub mode: Mode,
    pub paths: BTreeMap<String, Vec<PathPoint>>,
    pub fps: f64,
    pub geometry: Geometry,
    pub appeared: BTreeMap<String, i64>,
    pub crosshair: Vec<(f64, f64)>,
    pub run: Option<serde_json::Value>,
}

/// A review's results: the flicks matched to the kills (flicks.json) and the report.
pub struct Reviewed {
    pub flicks: Vec<Flick>,
    pub report: Report,
}

/// Reviews a clicking run from its tracks and its stats file's text. `video` and `stats` are the names the report
/// gives; `run` is the user's run marks, kept as given.
pub fn review_clicks(
    tracks: &Tracks,
    stats_text: &str,
    video: &str,
    stats: &str,
    run: Option<serde_json::Value>,
) -> Result<Reviewed, String> {
    let file = StatsFile::parse(stats_text);
    let kills = file.kills()?;
    let (flicks, mut info) = match_times(tracks, &kills.times, &kills.shots, 0.25, None);
    info.source = Some(KillSource::Stats);
    let mut per_kill = kills.shots.clone();
    per_kill.sort();
    if per_kill.is_empty() {
        per_kill.push(1);
    }
    let r = target_radius(&flicks);
    let ms = measure(&flicks, tracks.fps, r);
    let mode = if per_kill[per_kill.len() / 2] > 3 { Mode::Hold } else { Mode::Click };
    let ch = choices(tracks, &flicks);
    let summary = summarize(&ms, &ch, &file.meta, info, r, mode)?;
    let report = Report {
        video: video.into(),
        stats: Some(stats.into()),
        issues: judge(&summary),
        summary,
        flicks: ms,
        mode,
        paths: flicks.iter().map(|f| (f.n.to_string(), f.traj.clone())).collect(),
        fps: tracks.fps,
        geometry: Geometry { W, H, CX, CY, K },
        appeared: appearances(tracks, 0.5, 1.0).appeared.into_iter().map(|(t, i)| (t.to_string(), i)).collect(),
        crosshair: crosshair_spots(&tracks.frames),
        run,
    };
    Ok(Reviewed { flicks, report })
}

/// A tracking run's report, as report.json keeps it: the summary, with the clicking run's parts empty.
#[derive(Clone, Debug, Serialize)]
pub struct TrackReport {
    pub video: String,
    pub stats: Option<String>,
    pub summary: TrackSummary,
    pub issues: Vec<Issue>,
    pub flicks: Vec<Measure>,
    pub mode: Mode,
    pub paths: BTreeMap<String, Vec<PathPoint>>,
    pub fps: f64,
    pub geometry: Geometry,
    pub appeared: BTreeMap<String, i64>,
    pub crosshair: Vec<(f64, f64)>,
    pub run: Option<serde_json::Value>,
    pub limit: Option<f64>,
}

/// What a tracking run reads from its video besides the tracks: per frame the camera's reading, and whether KovaaK's
/// countdown bar shows.
pub struct VideoReadings<'a> {
    pub camera: &'a [CameraReading],
    pub countdown: &'a [bool],
}

/// Reviews a tracking run from its tracks, its stats file and its video's readings. `limit`: the scenario's time
/// limit (seconds), which the stats file's own length overrides.
pub fn review_tracking(
    tracks: &Tracks,
    stats_text: &str,
    video: &str,
    stats: &str,
    limit: Option<f64>,
    readings: VideoReadings,
    run: Option<serde_json::Value>,
) -> Result<TrackReport, String> {
    let file = StatsFile::parse(stats_text);
    let fps = tracks.fps;
    let limit = stats_length(stats, &file).or(limit);
    let (mut deaths, mut start) = (Vec::new(), None);
    if !file.rows.is_empty() {
        // bots that die: their kills, matched in the video, and the challenge's start on the video's clock
        let kills = file.kills()?;
        let (flicks, info) = match_times(tracks, &kills.times, &kills.shots, 0.25, None);
        deaths = flicks.iter().map(|f| f.kill_frame).collect();
        if let Some(off) = info.offset
            && info.matched > 0
        {
            start = Some((off * fps).round_ties_even() as i64);
        }
    }
    if start.is_none()
        && let Some(l) = limit.filter(|&l| l != 0.0)
    {
        // no kills to place the start: KovaaK's countdown ends it
        let until = (tracks.frames.len() as f64 / fps - l + 3.0).max(5.0);
        start = countdown_end(readings.countdown, fps, until).map(|i| i as i64);
    }
    // the user's own window comes first
    let mut limit = limit;
    if let Some(marks) = run.as_ref() {
        let (first, length) = run_window(marks, fps, limit);
        start = first.or(start);
        limit = length;
    }
    let summary = track_summary(tracks, &file.meta, limit, Some(readings.camera), &deaths, start, KillSource::Stats);
    Ok(TrackReport {
        video: video.into(),
        stats: Some(stats.into()),
        summary,
        issues: Vec::new(),
        flicks: Vec::new(),
        mode: Mode::Track,
        paths: BTreeMap::new(),
        fps,
        geometry: Geometry { W, H, CX, CY, K },
        appeared: BTreeMap::new(),
        crosshair: Vec::new(),
        run,
        limit,
    })
}

/// The user's run window (start, end, length in seconds, any of them null) as the first frame and the length in
/// seconds, or (None, limit) where it says nothing (python/review.py: `run_window`). Two of the three settle the third;
/// a start or an end alone takes the length given (the stats file's or the scenario's).
pub fn run_window(marks: &serde_json::Value, fps: f64, limit: Option<f64>) -> (Option<i64>, Option<f64>) {
    let get = |k: &str| marks.get(k).and_then(serde_json::Value::as_f64);
    let (a, b) = (get("start"), get("end"));
    let frame = |t: f64| (t * fps).round_ties_even() as i64;
    if let (Some(a), Some(b)) = (a, b)
        && b > a
    {
        return (Some(frame(a)), Some(b - a));
    }
    let length = get("length").filter(|&l| l != 0.0).or(limit.filter(|&l| l != 0.0));
    match (a, b, length) {
        (Some(a), _, _) => (Some(frame(a)), length),
        (None, Some(b), Some(l)) => (Some(frame((b - l).max(0.0))), length),
        _ => (None, length),
    }
}

/// What the page asks the core to review: the tracks, the video's name, the stats file's name and text, the user's
/// run marks; for a tracking run also the scenario's time limit and the video's readings.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewRequest {
    pub tracks: Tracks,
    pub video: String,
    pub stats: String,
    pub stats_text: String,
    #[serde(default)]
    pub run: Option<serde_json::Value>,
    #[serde(default)]
    pub tracking: bool,
    #[serde(default)]
    pub limit: Option<f64>,
    #[serde(default)]
    pub camera: Vec<CameraReading>,
    #[serde(default)]
    pub countdown: Vec<bool>,
}

/// A clicking run's report or a tracking run's.
#[derive(Serialize)]
#[serde(untagged)]
pub enum AnyReport {
    Click(Box<Report>),
    Track(Box<TrackReport>),
}

/// The report, or why there is none: {"report": ...} or {"error": "..."}.
#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Report(AnyReport),
    Error(String),
}

fn review_request(r: ReviewRequest) -> Result<AnyReport, String> {
    if r.tracking {
        let readings = VideoReadings { camera: &r.camera, countdown: &r.countdown };
        review_tracking(&r.tracks, &r.stats_text, &r.video, &r.stats, r.limit, readings, r.run)
            .map(|t| AnyReport::Track(Box::new(t)))
    } else {
        review_clicks(&r.tracks, &r.stats_text, &r.video, &r.stats, r.run).map(|c| AnyReport::Click(Box::new(c.report)))
    }
}

/// A request (JSON) reviewed, as JSON.
pub fn review_json(request: &[u8]) -> Vec<u8> {
    let outcome = serde_json::from_slice::<ReviewRequest>(request)
        .map_err(|e| format!("The review request could not be read: {e}"))
        .and_then(review_request)
        .map_or_else(Outcome::Error, Outcome::Report);
    serde_json::to_vec(&outcome).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// python/review.py's run_window: two marks settle the third; one takes the length given.
    #[test]
    fn the_run_window_as_python_reads_it() {
        let w = |m: serde_json::Value, limit| run_window(&m, 60.0, limit);
        assert_eq!(w(json!({"start": 2.0, "end": 12.0, "length": null}), Some(60.0)), (Some(120), Some(10.0)));
        assert_eq!(w(json!({"start": 2.0, "end": null, "length": 5.0}), Some(60.0)), (Some(120), Some(5.0)));
        assert_eq!(w(json!({"start": 2.0, "end": null, "length": null}), Some(60.0)), (Some(120), Some(60.0)));
        assert_eq!(w(json!({"start": null, "end": 12.0, "length": 5.0}), None), (Some(420), Some(5.0)));
        assert_eq!(w(json!({"start": null, "end": 3.0, "length": 5.0}), None), (Some(0), Some(5.0)));
        assert_eq!(w(json!({"start": null, "end": null, "length": null}), Some(60.0)), (None, Some(60.0)));
        assert_eq!(w(json!({"start": 9.0, "end": 3.0, "length": 0.0}), None), (Some(540), None));
    }
}
