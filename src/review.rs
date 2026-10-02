//! The review of a clicking run with its stats file (review.py: `review`, the clicking branch).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::geometry::{CX, CY, H, K, W};
use crate::matching::{appearances, crosshair_spots, match_times, Flick, KillSource, PathPoint};
use crate::measure::{choices, measure, target_radius, Measure};
use crate::stats_file::StatsFile;
use crate::summary::{judge, summarize, Issue, Mode, Summary};
use crate::track::Tracks;

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

/// What the page asks the core to review: the tracks, the stats file's text and name, the video's name and the user's
/// run marks.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClickRequest {
    pub tracks: Tracks,
    pub stats_text: String,
    pub video: String,
    pub stats: String,
    #[serde(default)]
    pub run: Option<serde_json::Value>,
}

/// The report, or why there is none: {"report": ...} or {"error": "..."}.
#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Report(Box<Report>),
    Error(String),
}

/// A request (JSON) reviewed, as JSON.
pub fn review_json(request: &[u8]) -> Vec<u8> {
    let outcome = serde_json::from_slice::<ClickRequest>(request)
        .map_err(|e| format!("The review request could not be read: {e}"))
        .and_then(|r| review_clicks(&r.tracks, &r.stats_text, &r.video, &r.stats, r.run))
        .map_or_else(Outcome::Error, |r| Outcome::Report(Box::new(r.report)));
    serde_json::to_vec(&outcome).unwrap_or_default()
}
