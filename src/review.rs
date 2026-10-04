//! The review of a run (review.py: `review`): a clicking run's flicks, or a tracking run's time on the target. The
//! kills come from the run's stats file; without one, from the HUD read in the video (src/hud.rs); without a readable
//! HUD, from the video alone.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use crate::capped::Capped;
use crate::faint::{FaintSetting, without_faint};
use crate::geometry::{CX, CY, H, K, W};
use crate::hud::{HudGame, HudReading};
use crate::matching::{
    appearances, crosshair_spots, match_times, match_video, without_ghosts, Flick, KillSource, MatchInfo, PathPoint,
    SPOTS,
};
use crate::measure::{choices, measure, target_radius, Measure};
use crate::reload::reload_cost;
use crate::scenario::AmmoRules;
use crate::stats_file::StatsFile;
use crate::summary::{judge, summarize, Issue, Mode, Summary};
use crate::track::{REVIEW_VERSION, Tracks};
use crate::tracking::{CameraReading, FaintCut, TrackSummary, countdown_end, stats_length, track_summary};

/// The frame's size and the crosshair's place (pixels), and the focal length (pixels) the degrees come from.
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, rename = "ClickReport"))]
pub struct Report {
    pub video: String,
    pub stats: Option<String>,
    pub summary: Summary,
    pub issues: Vec<Issue>,
    pub flicks: Vec<Measure>,
    #[cfg_attr(feature = "ts", ts(type = r#""click" | "hold""#))]
    pub mode: Mode,
    #[cfg_attr(feature = "ts", ts(as = "BTreeMap<String, Vec<crate::typescript::PathPoint>>"))]
    pub paths: BTreeMap<String, Vec<PathPoint>>,
    pub fps: f64,
    pub geometry: Geometry,
    pub appeared: BTreeMap<String, i64>,
    #[cfg_attr(feature = "ts", ts(as = "Vec<crate::typescript::CrosshairSpot>"))]
    pub crosshair: Capped<(f64, f64), SPOTS>,
    /// The user's run marks as given ({start, end, length}: the service's RunMarks).
    #[cfg_attr(feature = "ts", ts(type = "unknown"))]
    pub run: Option<serde_json::Value>,
    /// Kept by an older version of the review (`REVIEW_VERSION`): review again for what it lacks.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    #[cfg_attr(feature = "ts", ts(as = "Option<bool>", optional))]
    pub outdated: bool,
}

/// A review's results: the flicks matched to the kills (flicks.json) and the report.
pub struct Reviewed {
    pub flicks: Vec<Flick>,
    pub report: Report,
}

/// Where a run's kills come from: its stats file (its name and text), or, for a run without one, the HUD read in the
/// video (None where it did not read).
#[derive(Clone, Copy)]
pub enum KillTimes<'a> {
    Stats { name: &'a str, text: &'a str },
    Unpaired { hud: Option<&'a HudReading> },
}

/// The scenario and the score a recording's name gives ("<scenario> - <score> - <time>"); other recorders name files
/// freely, so either can be missing.
fn name_parts(video: &str) -> (String, Option<String>) {
    let stem = std::path::Path::new(video).file_stem().map_or(Cow::Borrowed(video), |s| s.to_string_lossy());
    let mut parts: Capped<&str, 3> = stem.rsplitn(3, " - ").collect();
    parts.reverse();
    let score = parts.get(1).and_then(|s| {
        let s = s.trim_start();
        let digits = |t: &str| t.len() - t.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        let whole = digits(s);
        let frac = s[whole..].strip_prefix('.').map_or(0, |r| match digits(r) {
            0 => 0,
            d => d + 1,
        });
        (whole > 0).then(|| s[..whole + frac].to_string())
    });
    (parts[0].to_string(), score)
}

/// The shots each HUD kill took. The Accuracy row can update up to a third of a second after the Kill Count, so a shot
/// can land in the next kill's stretch: in one-hit scenarios (hits about equal to kills) each kill is one hit plus the
/// misses in its stretch; otherwise the shots in its stretch.
fn hud_shots(h: &HudReading) -> Vec<i64> {
    let count = |frames: &[i64]| {
        let mut c: BTreeMap<i64, i64> = BTreeMap::new();
        for &f in frames {
            *c.entry(f).or_default() += 1;
        }
        c
    };
    let (shots, hits) = (count(&h.shots), count(&h.hits));
    let spans = std::iter::once(-1).chain(h.kills.iter().copied()).zip(h.kills.iter().copied());
    // two kills on one frame (the count went up by two) leave the second an empty stretch
    let within =
        |c: &BTreeMap<i64, i64>, a: i64, b: i64| if a < b { c.range(a + 1..=b).map(|(_, &n)| n).sum() } else { 0 };
    if !h.hits.is_empty() && h.hits.len() as f64 <= 1.2 * h.kills.len() as f64 {
        let miss: BTreeMap<i64, i64> = shots
            .iter()
            .map(|(&f, &n)| (f, n - hits.get(&f).copied().unwrap_or(0)))
            .filter(|&(_, n)| n > 0)
            .collect();
        spans.map(|(a, b)| 1 + within(&miss, a, b)).collect()
    } else {
        spans.map(|(a, b)| within(&shots, a, b)).collect()
    }
}

/// The run's hits a kill (the Hit Count over the Kills), rounded, at least 1.
fn hits_per_kill(meta: &HashMap<String, String>) -> Option<i64> {
    let get = |key: &str| meta.get(key)?.trim().parse::<f64>().ok();
    let (hits, kills) = (get("Hit Count")?, get("Kills")?);
    (kills > 0.0).then(|| ((hits / kills).round() as i64).max(1))
}

/// Reviews a clicking run from its tracks and its kill times. `video` is the recording's name (the report gives it,
/// and without a stats file the scenario and the score come from it); `run` is the user's run marks, kept as given;
/// `reload` the ammo rules of the scenario's weapon, when its magazine runs out (src/reload.rs works out the reloads
/// it forced from each kill's shots).
pub fn review_clicks(
    tracks: &Tracks,
    kills: KillTimes,
    video: &str,
    run: Option<serde_json::Value>,
    reload: Option<&AmmoRules>,
) -> Result<Reviewed, String> {
    let (flicks, info, meta, mut per_kill, hits, stats) = match kills {
        KillTimes::Stats { name, text } => {
            let file = StatsFile::parse(text);
            let kills = file.kills()?;
            let (flicks, mut info) = match_times(tracks, &kills.times, &kills.shots, 0.25, None);
            info.source = Some(KillSource::Stats);
            let hits = file.hits();
            (flicks, info, file.meta, kills.shots, hits, Some(name))
        }
        KillTimes::Unpaired { hud } => {
            let (flicks, info, meta, per_kill) = unpaired_kills(tracks, hud, video);
            (flicks, info, meta, per_kill, None, None)
        }
    };
    // each kill's shots in the run's order; its hits from the stats file's kill table, else the run's hits a kill
    let cost = reload.filter(|_| !per_kill.is_empty()).map(|rules| {
        let n = per_kill.len();
        let hits = hits.filter(|h| h.len() == n).or_else(|| hits_per_kill(&meta).map(|h| vec![h; n]));
        reload_cost(rules, &per_kill, hits.as_deref())
    });
    per_kill.sort();
    if per_kill.is_empty() {
        per_kill.push(1);
    }
    let r = target_radius(&flicks);
    let mut ms = measure(&flicks, tracks, r);
    if let Some(c) = &cost {
        for m in &mut ms {
            if let Some(k) = m.kill_number.checked_sub(1).and_then(|i| c.per_kill.get(i)) {
                (m.reloads, m.reload_time) = (Some(k.reloads), Some(k.seconds));
            }
        }
    }
    let mode = if per_kill[per_kill.len() / 2] > 3 { Mode::Hold } else { Mode::Click };
    let ch = choices(tracks, &flicks);
    let summary = summarize(&ms, &ch, &meta, info, r, mode, cost.as_ref())?;
    let report = Report {
        video: video.into(),
        stats: stats.map(Into::into),
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
        outdated: false,
    };
    Ok(Reviewed { flicks, report })
}

/// A clicking run's kills without a stats file: from the HUD, with its shots and totals (the score from the file name,
/// or Aim Lab's points), else from the video alone (no score, shots, misses or accuracy). Returns the flicks, how they
/// matched, the stats file's facts the HUD gives, and each kill's shots.
fn unpaired_kills(
    tracks: &Tracks,
    hud: Option<&HudReading>,
    video: &str,
) -> (Vec<Flick>, MatchInfo, HashMap<String, String>, Vec<i64>) {
    let (scenario, score) = name_parts(video);
    let mut meta = HashMap::from([("Scenario".to_string(), scenario)]);
    // Aim Lab counts hits: in a task whose targets take several hits, the video's kills are the kills
    let hud = hud.filter(|h| {
        !h.kills.is_empty()
            && (h.game != HudGame::Aimlab || h.kills.len() as f64 <= 1.3 * match_video(tracks).0.len() as f64)
    });
    let Some(h) = hud else {
        let (flicks, info) = match_video(tracks);
        return (flicks, info, meta, Vec::new());
    };
    let shots = hud_shots(h);
    // Aim Lab's crosshair is marked as a target by every model: its tracks were taken for the killed target. In
    // KovaaK's runs they stay: a target held under the crosshair looks like them
    let aimlab = h.game == HudGame::Aimlab;
    let t = if aimlab { Cow::Owned(without_ghosts(tracks)) } else { Cow::Borrowed(tracks) };
    let times: Vec<f64> = h.kills.iter().map(|&f| f as f64 / tracks.fps).collect();
    let (flicks, mut info) = match_times(&t, &times, &shots, 0.25, Some(0.0));
    info.source = Some(if aimlab { KillSource::Aimlab } else { KillSource::Hud });
    meta.insert("Kills".into(), h.totals.kills.to_string());
    if let Some(s) = score.or_else(|| h.points.map(|p| p.to_string())) {
        meta.insert("Score".into(), s);
    }
    if let (Some(hits), Some(all)) = (h.totals.hits, h.totals.shots) {
        meta.insert("Hit Count".into(), hits.to_string());
        meta.insert("Miss Count".into(), (all - hits).to_string());
    }
    (flicks, info, meta, shots)
}

/// A tracking run's report, as report.json keeps it: the summary, with the clicking run's parts empty.
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TrackReport {
    pub video: String,
    pub stats: Option<String>,
    pub summary: TrackSummary,
    pub issues: Vec<Issue>,
    pub flicks: Vec<Measure>,
    #[cfg_attr(feature = "ts", ts(type = r#""track""#))]
    pub mode: Mode,
    #[cfg_attr(feature = "ts", ts(as = "BTreeMap<String, Vec<crate::typescript::PathPoint>>"))]
    pub paths: BTreeMap<String, Vec<PathPoint>>,
    pub fps: f64,
    pub geometry: Geometry,
    pub appeared: BTreeMap<String, i64>,
    #[cfg_attr(feature = "ts", ts(as = "Vec<crate::typescript::CrosshairSpot>"))]
    pub crosshair: Capped<(f64, f64), SPOTS>,
    #[cfg_attr(feature = "ts", ts(type = "unknown"))]
    pub run: Option<serde_json::Value>,
    pub limit: Option<f64>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    #[cfg_attr(feature = "ts", ts(as = "Option<bool>", optional))]
    pub outdated: bool,
}

/// What a tracking run reads from its video besides the tracks: per frame the camera's reading, and whether KovaaK's
/// countdown bar shows.
pub struct VideoReadings<'a> {
    pub camera: &'a [CameraReading],
    pub countdown: &'a [bool],
}

/// Reviews a tracking run from its tracks, its kill times (bots that die) and its video's readings. `limit`: the
/// scenario's time limit (seconds), which the stats file's own length overrides. `faint`: the user's faint-target
/// cut-off; when on, the measures leave out the tracks it cuts (the kills are still matched on every track).
pub fn review_tracking(
    tracks: &Tracks,
    kills: KillTimes,
    video: &str,
    limit: Option<f64>,
    readings: VideoReadings,
    run: Option<serde_json::Value>,
    faint: Option<FaintSetting>,
) -> Result<TrackReport, String> {
    let fps = tracks.fps;
    let (mut deaths, mut start, mut limit) = (Vec::new(), None, limit);
    let (meta, stats, source) = match kills {
        KillTimes::Stats { name, text } => {
            let file = StatsFile::parse(text);
            limit = stats_length(name, &file).or(limit);
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
            (file.meta, Some(name), KillSource::Stats)
        }
        KillTimes::Unpaired { hud } => {
            let meta = HashMap::from([("Scenario".to_string(), name_parts(video).0)]);
            match hud.filter(|h| h.game == HudGame::Kovaak && !h.kills.is_empty()) {
                Some(h) => {
                    deaths = h.kills.clone();
                    (meta, None, KillSource::Hud)
                }
                None => (meta, None, KillSource::Video),
            }
        }
    };
    if start.is_none()
        && let Some(l) = limit.filter(|&l| l != 0.0)
    {
        // no kills to place the start: KovaaK's countdown ends it
        let until = (tracks.frames.len() as f64 / fps - l + 3.0).max(5.0);
        start = countdown_end(readings.countdown, fps, until).map(|i| i as i64);
    }
    // the user's own window comes first
    if let Some(marks) = run.as_ref() {
        let (first, length) = run_window(marks, fps, limit);
        start = first.or(start);
        limit = length;
    }
    let cut_tracks: Tracks;
    let (measured, cut) = match faint.filter(|f| f.on) {
        Some(f) => {
            let c = without_faint(&tracks.frames, f.offset, 0.0);
            cut_tracks = Tracks { fps, frames: c.frames, version: tracks.version };
            (&cut_tracks, Some(FaintCut { offset: f.offset, cut: c.cut, tracks: c.gone }))
        }
        None => (tracks, None),
    };
    let mut summary = track_summary(measured, &meta, limit, Some(readings.camera), &deaths, start, source);
    summary.faint = cut;
    Ok(TrackReport {
        video: video.into(),
        stats: stats.map(Into::into),
        summary,
        issues: Vec::new(),
        flicks: Vec::new(),
        mode: Mode::Track,
        paths: BTreeMap::new(),
        fps,
        geometry: Geometry { W, H, CX, CY, K },
        appeared: BTreeMap::new(),
        crosshair: Capped::new(),
        run,
        limit,
        outdated: false,
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

/// What the page asks the core to review: the tracks, the video's name, the stats file's name and text (empty without
/// one), what the HUD read, the user's run marks; for a clicking run the ammo rules of the scenario's weapon (null or
/// missing: its magazine never runs out, or the scenario is not known); for a tracking run also the scenario's time
/// limit, the video's readings and the user's faint-target cut-off ({on, offset}; null or missing: none).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewRequest {
    pub tracks: Tracks,
    pub video: String,
    #[serde(default)]
    pub stats: String,
    #[serde(default)]
    pub stats_text: String,
    #[serde(default)]
    pub hud: Option<HudReading>,
    #[serde(default)]
    pub run: Option<serde_json::Value>,
    #[serde(default)]
    pub tracking: bool,
    #[serde(default)]
    pub limit: Option<f64>,
    #[serde(default)]
    pub reload: Option<AmmoRules>,
    #[serde(default)]
    pub camera: Vec<CameraReading>,
    #[serde(default)]
    pub countdown: Vec<bool>,
    #[serde(default)]
    pub faint: Option<FaintSetting>,
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
    let kills = if r.stats_text.is_empty() {
        KillTimes::Unpaired { hud: r.hud.as_ref() }
    } else {
        KillTimes::Stats { name: &r.stats, text: &r.stats_text }
    };
    let outdated = r.tracks.version < REVIEW_VERSION;
    if r.tracking {
        let readings = VideoReadings { camera: &r.camera, countdown: &r.countdown };
        let t = review_tracking(&r.tracks, kills, &r.video, r.limit, readings, r.run, r.faint)?;
        Ok(AnyReport::Track(Box::new(TrackReport { outdated, ..t })))
    } else {
        let c = review_clicks(&r.tracks, kills, &r.video, r.run, r.reload.as_ref())?;
        Ok(AnyReport::Click(Box::new(Report { outdated, ..c.report })))
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
