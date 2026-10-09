//! The review of a run (python/retired/review.py: `review`): a clicking run's flicks, or a tracking run's time on the
//! target. The kills come from the run's stats file; without one, from the HUD read in the video (src/hud/); without
//! a readable HUD, from the video alone. In: a request (`review_json`: the run's tracks, its stats file, what the video
//! read and the user's run marks), which the service builds in every mode (service/src/report.rs). Out: the report as
//! JSON (report.json), which the run page shows.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::RangeInclusive;

use serde::{Deserialize, Serialize};

use crate::capped::Capped;
use crate::faint::{FaintSetting, without_faint};
use crate::geometry::{CX, CY, H, K, W};
use crate::hud::{HudFinal, HudGame, HudReading};
use crate::kill_check::{KillEvidence, ruled_out, with_hidden_kills};
use crate::matching::{
    Flick, JOIN_GAP_S, JOIN_RADIUS_DEG, KillSource, MatchInfo, PathPoint, SPOTS, appearances, crosshair_spots,
    match_times, match_video, without_ghosts,
};
use crate::measure::{Measure, choices, measure, target_radius};
use crate::reload::{ReloadCost, reload_cost};
use crate::scenario::{AmmoRules, Hitbox};
use crate::stats_file::StatsFile;
use crate::summary::{Issue, Mode, Summary, judge, summarize};
use crate::track::{REVIEW_VERSION, Tracks};
use crate::tracking::{CameraReading, FaintCut, RunFacts, TrackSummary, countdown_end, stats_length, track_summary};

/// A kill's target is the track nearest the crosshair in this many seconds before it (`match_times`' window).
const KILL_WINDOW_S: f64 = 0.25;
/// The HUD's kills are on the video's clock already (`match_times`' offset).
const ON_VIDEO_CLOCK: Option<f64> = Some(0.0);
/// The separator between the parts of a recording's name: "<scenario> - <score> - <time>".
const NAME_SEPARATOR: &str = " - ";
/// The parts of a recording's name: scenario, score and time.
const NAME_FIELDS: usize = 3;
/// A HUD with at most this many hits a kill is a one-hit scenario's: each kill took one hit.
const ONE_HIT_MAX_HITS_PER_KILL: f64 = 1.2;
/// Aim Lab's HUD counts hits: with more than this many of its kills to each kill the video finds, its targets take
/// several hits, and the video's kills are the kills.
const AIMLAB_MAX_KILLS_PER_VIDEO_KILL: f64 = 1.3;
/// A run whose median kill took more shots than this holds the trigger.
const MAX_CLICK_SHOTS: i64 = 3;
/// KovaaK's countdown is looked for up to this many seconds past the latest the run can start (the recording's length
/// less the run's), and over the first `MIN_COUNTDOWN_SEARCH_S` seconds at least.
const COUNTDOWN_SLACK_S: f64 = 3.0;
/// The countdown is always looked for over at least this many seconds from the recording's start.
const MIN_COUNTDOWN_SEARCH_S: f64 = 5.0;
/// A tracking run's faint scores count the frames under the crosshair too: its bot is there most of the time.
const TRACKING_FAINT_NEAR_DEG: f64 = 0.0;

/// The frame's size and the crosshair's place (pixels), and the focal length (pixels) the degrees come from.
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[allow(non_snake_case)]
#[expect(clippy::min_ident_chars, reason = "W, H and K are the JSON's keys, which the run page reads")]
pub struct Geometry {
    /// The frame's width (pixels).
    pub W: usize,
    /// The frame's height (pixels).
    pub H: usize,
    /// The crosshair's x (pixels from the left).
    pub CX: f64,
    /// The crosshair's y (pixels from the top).
    pub CY: f64,
    /// The focal length (pixels) for a 103 degree horizontal FOV: a place `x` pixels right of the crosshair is
    /// atan(x / K) to its right.
    pub K: f64,
}

/// The geometry every report gives: the frame's (1280 x 720).
const FRAME_GEOMETRY: Geometry = Geometry { W, H, CX, CY, K };

/// A clicking run's report, as report.json keeps it.
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, rename = "ClickReport"))]
pub struct Report {
    /// The recording's name, as the request gave it.
    pub video: String,
    /// The stats file's name; None for a run without one.
    pub stats: Option<String>,
    /// The run's medians and shares (src/summary.rs).
    pub summary: Summary,
    /// Each check's verdict on the summary (`judge`).
    pub issues: Vec<Issue>,
    /// Each matched kill's measures (src/measure.rs), in kill order.
    pub flicks: Vec<Measure>,
    /// Whether the run clicks or holds the trigger (`click_mode`).
    #[cfg_attr(feature = "ts", ts(type = r#""click" | "hold""#))]
    pub mode: Mode,
    /// Each matched kill's target path, under its kill number (as text).
    #[cfg_attr(feature = "ts", ts(as = "BTreeMap<String, Vec<crate::typescript::PathPoint>>"))]
    pub paths: BTreeMap<String, Vec<PathPoint>>,
    /// The video's frame rate (frames a second).
    pub fps: f64,
    /// The frame's size, the crosshair's place and the focal length, for the page to turn degrees into pixels.
    pub geometry: Geometry,
    /// Each track's id (as text) with the frame its target first appeared on (`appeared`).
    pub appeared: BTreeMap<String, i64>,
    /// Where the detector marks the crosshair as a target (degrees), if it does (`crosshair_spots`).
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
    /// The flicks matched to the kills, as flicks.json keeps them.
    pub flicks: Vec<Flick>,
    /// The report, as report.json keeps it.
    pub report: Report,
}

/// Where a run's kills come from: its stats file (its name and text), or, for a run without one, the HUD read in the
/// video (None where it did not read) and, for the kills the video alone gives, their check in the frames round them
/// (kill_check.rs; None: not checked).
#[derive(Clone, Copy)]
pub enum KillTimes<'a> {
    /// The run's stats file: its name (`name`) and its CSV text (`text`).
    Stats {
        /// The stats file's name, which holds the scenario and the time the run ended.
        name: &'a str,
        /// The stats file's CSV text.
        text: &'a str,
    },
    /// No stats file: what the HUD read (`hud`), and the check of the video's kills (`checked`).
    Unpaired {
        /// What the HUD read in the video; None where it did not read.
        hud: Option<&'a HudReading>,
        /// The check of the kills the video alone gives, in the frames round them; None when not checked.
        checked: Option<&'a [KillEvidence]>,
    },
}

/// The number a text starts with (digits, then a point and digits), as written; None where it starts with no digit.
fn leading_number(text: &str) -> Option<String> {
    let digits = |part: &str| part.len() - part.trim_start_matches(|character: char| character.is_ascii_digit()).len();
    let whole = digits(text);
    let fraction = text[whole..].strip_prefix('.').map_or(0, |rest| match digits(rest) {
        0 => 0,
        count => count + 1,
    });
    (whole > 0).then(|| text[..whole + fraction].to_string())
}

/// The scenario and the score a recording's name gives ("<scenario> - <score> - <time>"); other recorders name files
/// freely, so either can be missing.
fn name_parts(video: &str) -> (String, Option<String>) {
    let stem = std::path::Path::new(video).file_stem().map_or(Cow::Borrowed(video), |stem| stem.to_string_lossy());
    let mut parts: Capped<&str, NAME_FIELDS> = stem.rsplitn(NAME_FIELDS, NAME_SEPARATOR).collect();
    parts.reverse();
    let score = parts.get(1).and_then(|part| leading_number(part.trim_start()));
    (parts[0].to_string(), score)
}

/// How many times each frame comes up.
fn per_frame(frames: &[i64]) -> BTreeMap<i64, i64> {
    let mut counts: BTreeMap<i64, i64> = BTreeMap::new();
    for &frame in frames {
        *counts.entry(frame).or_default() += 1;
    }
    counts
}

/// The counts in a kill's stretch: the frames after the kill before (`a`) up to this kill (`b`). Two kills on one
/// frame (the count went up by two) leave the second an empty stretch.
fn within(counts: &BTreeMap<i64, i64>, a: i64, b: i64) -> i64 {
    if a < b { counts.range(a + 1..=b).map(|(_, &count)| count).sum() } else { 0 }
}

/// The shots each HUD kill took. The Accuracy row can update up to a third of a second after the Kill Count, so a shot
/// can land in the next kill's stretch: in one-hit scenarios (hits about equal to kills) each kill is one hit plus the
/// misses in its stretch; otherwise the shots in its stretch.
fn hud_shots(hud: &HudReading) -> Vec<i64> {
    let (shots, hits) = (per_frame(&hud.shots), per_frame(&hud.hits));
    // the first kill's stretch starts before the first frame
    let spans = std::iter::once(-1).chain(hud.kills.iter().copied()).zip(hud.kills.iter().copied());
    if !hud.hits.is_empty() && hud.hits.len() as f64 <= ONE_HIT_MAX_HITS_PER_KILL * hud.kills.len() as f64 {
        let misses: BTreeMap<i64, i64> = shots
            .iter()
            .map(|(&frame, &count)| (frame, count - hits.get(&frame).copied().unwrap_or(0)))
            .filter(|&(_, count)| count > 0)
            .collect();
        spans.map(|(a, b)| 1 + within(&misses, a, b)).collect()
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

/// What a clicking run's kill times give: the flicks matched to them, how they matched, the stats file's facts (or
/// what the HUD and the file name give), each kill's shots in the run's order, its hits (only from a stats file's kill
/// table) and the stats file's name.
struct ClickKills<'a> {
    /// The flicks matched to the kills.
    flicks: Vec<Flick>,
    /// How the kills matched: the counts, the clock offset and where the kills came from.
    info: MatchInfo,
    /// The stats file's "Key:,value" facts by key, or the ones the HUD and the file name give.
    meta: HashMap<String, String>,
    /// Each kill's shots, in the run's order; empty when nothing gave them (the video alone).
    shots: Vec<i64>,
    /// Each kill's hits, in the run's order; only a stats file's kill table gives them.
    hits: Option<Vec<i64>>,
    /// The stats file's name; None without one.
    stats: Option<&'a str>,
}

/// A clicking run's kills from its stats file (`name`, `text`); with a run window, only those inside it (each kill on
/// the video's clock by the matching's offset).
fn stats_kills<'a>(
    tracks: &Tracks,
    name: &'a str,
    text: &str,
    window: Option<&RangeInclusive<i64>>,
) -> Result<ClickKills<'a>, String> {
    let file = StatsFile::parse(text);
    let kills = file.kills()?;
    let (flicks, mut info) = match_times(tracks, &kills.times, &kills.shots, KILL_WINDOW_S, None);
    info.source = Some(KillSource::Stats);
    let hits = file.hits();
    let all = ClickKills { flicks, info, meta: file.meta, shots: kills.shots, hits, stats: Some(name) };
    Ok(match (window, all.info.offset) {
        (Some(window), Some(offset_s)) => {
            let frame = |time_s: &f64| Some(((time_s + offset_s) * tracks.fps).round_ties_even() as i64);
            let kill_frames: Vec<Option<i64>> = kills.times.iter().map(frame).collect();
            kills_within(all, &kill_frames, window)
        }
        _ => all,
    })
}

/// The frames a clicking run is measured on (first..=last): the user's run window, when its marks give a start or an
/// end (`run_window`). A start alone runs to the recording's last frame, an end alone from its first.
fn click_window(marks: &serde_json::Value, fps: f64, frame_count: usize) -> Option<RangeInclusive<i64>> {
    let frame = |time_s: f64| (time_s * fps).round_ties_even() as i64;
    let last_frame = frame_count as i64 - 1;
    match run_window(marks, fps, None) {
        (Some(first), length_s) => Some(first..=length_s.map_or(last_frame, |length_s| first + frame(length_s))),
        (None, _) => marks.get("end").and_then(serde_json::Value::as_f64).map(|end_s| 0..=frame(end_s)),
    }
}

/// What the HUD read inside the window: its kills, hits and shots there, with the totals counted from them (Aim Lab's
/// points are the whole run's, so they are left out).
fn hud_within(reading: &HudReading, window: &RangeInclusive<i64>) -> HudReading {
    let inside =
        |frames: &[i64]| -> Vec<i64> { frames.iter().copied().filter(|frame| window.contains(frame)).collect() };
    let (kills, hits, shots) = (inside(&reading.kills), inside(&reading.hits), inside(&reading.shots));
    let totals = HudFinal {
        kills: kills.len() as i64,
        hits: reading.totals.hits.map(|_| hits.len() as i64),
        shots: reading.totals.shots.map(|_| shots.len() as i64),
    };
    HudReading { game: reading.game, kills, shots, hits, totals, checked: reading.checked, points: None }
}

/// Only the kills inside the window, numbered again from 1 (`kill_frames`: each kill's frame in the run's order, None
/// where it is not known): their flicks, shots and hits, and the counts the summary reads (the matched kills, and the
/// stats file's Kills, Hit Count and Miss Count from the kept kills' shots and hits).
fn kills_within<'a>(
    kills: ClickKills<'a>,
    kill_frames: &[Option<i64>],
    window: &RangeInclusive<i64>,
) -> ClickKills<'a> {
    let ClickKills { flicks, mut info, mut meta, shots, hits, stats } = kills;
    let kept: Vec<usize> =
        (0..kill_frames.len()).filter(|&i| kill_frames[i].is_some_and(|frame| window.contains(&frame))).collect();
    let new_number: HashMap<usize, usize> = kept.iter().enumerate().map(|(at, &i)| (i + 1, at + 1)).collect();
    let flicks: Vec<Flick> = flicks
        .into_iter()
        .filter_map(|flick| Some(Flick { kill_number: *new_number.get(&flick.kill_number)?, ..flick }))
        .collect();
    // each kill's shots and hits come one a kill, in the run's order
    let pick = |all: &[i64]| -> Vec<i64> {
        if all.len() == kill_frames.len() { kept.iter().map(|&i| all[i]).collect() } else { Vec::new() }
    };
    let (shots, hits) = (pick(&shots), hits.map(|hits| pick(&hits)));
    info.matched = flicks.len();
    if info.kills_stats.is_some() {
        info.kills_stats = Some(kept.len());
        meta.insert("Kills".into(), kept.len().to_string());
        match hits.as_ref().filter(|hits| hits.len() == kept.len()) {
            Some(hits) => {
                let (hit_count, shot_count) = (hits.iter().sum::<i64>(), shots.iter().sum::<i64>());
                meta.insert("Hit Count".into(), hit_count.to_string());
                meta.insert("Miss Count".into(), (shot_count - hit_count).max(0).to_string());
            }
            None => {
                meta.remove("Hit Count");
                meta.remove("Miss Count");
            }
        }
    } else {
        info.kills_video = kept.len();
    }
    ClickKills { flicks, info, meta, shots, hits, stats }
}

/// What reloading cost the run, for a scenario whose magazine runs out (`reload`): from each kill's shots in the run's
/// order, and its hits from the stats file's kill table (`hits`), else the run's hits a kill.
fn run_reload_cost(
    reload: Option<&AmmoRules>,
    shots: &[i64],
    hits: Option<Vec<i64>>,
    meta: &HashMap<String, String>,
) -> Option<ReloadCost> {
    reload.filter(|_| !shots.is_empty()).map(|rules| {
        let count = shots.len();
        let hits = hits
            .filter(|kill_hits| kill_hits.len() == count)
            .or_else(|| hits_per_kill(meta).map(|per_kill| vec![per_kill; count]));
        reload_cost(rules, shots, hits.as_deref())
    })
}

/// Each measured kill's reloads and their time, from the reloads its magazine forced.
fn add_reloads(measures: &mut [Measure], cost: &ReloadCost) {
    for kill in measures {
        if let Some(forced) = kill.kill_number.checked_sub(1).and_then(|i| cost.per_kill.get(i)) {
            (kill.reloads, kill.reload_time) = (Some(forced.reloads), Some(forced.seconds));
        }
    }
}

/// Click or hold, from each kill's shots: a run whose median kill took more than 3 holds the trigger.
fn click_mode(shots: &[i64]) -> Mode {
    let mut sorted = shots.to_vec();
    sorted.sort();
    if sorted.is_empty() {
        sorted.push(1);
    }
    if sorted[sorted.len() / 2] > MAX_CLICK_SHOTS { Mode::Hold } else { Mode::Click }
}

/// Each track (its id) with the frame its target first appeared on.
fn appeared(tracks: &Tracks) -> BTreeMap<String, i64> {
    let joined = appearances(tracks, JOIN_GAP_S, JOIN_RADIUS_DEG);
    joined.appeared.into_iter().map(|(id, frame)| (id.to_string(), frame)).collect()
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
    // the user's run window: the kills outside it are left out
    let window = run.as_ref().and_then(|marks| click_window(marks, tracks.fps, tracks.frames.len()));
    let kills = match kills {
        KillTimes::Stats { name, text } => stats_kills(tracks, name, text, window.as_ref())?,
        KillTimes::Unpaired { hud, checked } => unpaired_kills(tracks, hud, checked, video, window.as_ref()),
    };
    let ClickKills { flicks, info, meta, shots, hits, stats } = kills;
    let cost = run_reload_cost(reload, &shots, hits, &meta);
    let mode = click_mode(&shots);
    let radius = target_radius(&flicks);
    let mut measures = measure(&flicks, tracks, radius);
    if let Some(cost) = &cost {
        add_reloads(&mut measures, cost);
    }
    let summary = summarize(&measures, &choices(tracks, &flicks), &meta, info, radius, mode, cost.as_ref())?;
    let report = Report {
        video: video.into(),
        stats: stats.map(Into::into),
        issues: judge(&summary),
        summary,
        flicks: measures,
        mode,
        paths: flicks.iter().map(|flick| (flick.kill_number.to_string(), flick.path.clone())).collect(),
        fps: tracks.fps,
        geometry: FRAME_GEOMETRY,
        appeared: appeared(tracks),
        crosshair: crosshair_spots(&tracks.frames),
        run,
        outdated: false,
    };
    Ok(Reviewed { flicks, report })
}

/// The video's kills less those their check in the frames round them rules out (kill_check.rs: the target still
/// showed after), those whose target was hidden under the crosshair moved to when it died, the rest numbered again in
/// order; all of them as they were when they were not checked.
fn confirmed(
    tracks: &Tracks,
    (flicks, mut info): (Vec<Flick>, MatchInfo),
    checked: Option<&[KillEvidence]>,
) -> (Vec<Flick>, MatchInfo) {
    let Some(checked) = checked else { return (flicks, info) };
    let out: HashSet<i64> =
        checked.iter().filter(|evidence| ruled_out(evidence)).map(|evidence| evidence.frame).collect();
    let before = flicks.len();
    let kept: Vec<Flick> = flicks.into_iter().filter(|flick| !out.contains(&flick.kill_frame)).collect();
    let flicks: Vec<Flick> = with_hidden_kills(kept, checked, tracks)
        .into_iter()
        .enumerate()
        .map(|(at, flick)| Flick { kill_number: at + 1, ..flick })
        .collect();
    info.kills_video -= (before - flicks.len()).min(info.kills_video);
    info.matched = flicks.len();
    (flicks, info)
}

/// The stats file's facts the HUD gives: the kills, the score (from the file name, else Aim Lab's points) and the hits
/// and misses.
fn add_hud_facts(meta: &mut HashMap<String, String>, hud: &HudReading, score: Option<String>) {
    meta.insert("Kills".into(), hud.totals.kills.to_string());
    if let Some(score) = score.or_else(|| hud.points.map(|points| points.to_string())) {
        meta.insert("Score".into(), score);
    }
    if let (Some(hits), Some(all)) = (hud.totals.hits, hud.totals.shots) {
        meta.insert("Hit Count".into(), hits.to_string());
        meta.insert("Miss Count".into(), (all - hits).to_string());
    }
}

/// A clicking run's kills without a stats file: from the HUD, with its shots and totals (the score from the file name,
/// or Aim Lab's points), else from the video alone (no score, shots, misses or accuracy); with a run window, only the
/// kills inside it.
fn unpaired_kills(
    tracks: &Tracks,
    hud: Option<&HudReading>,
    checked: Option<&[KillEvidence]>,
    video: &str,
    window: Option<&RangeInclusive<i64>>,
) -> ClickKills<'static> {
    let (scenario, score) = name_parts(video);
    let mut meta = HashMap::from([("Scenario".to_string(), scenario)]);
    let windowed = hud.zip(window).map(|(reading, window)| hud_within(reading, window));
    let hud = windowed.as_ref().or(hud).filter(|reading| {
        !reading.kills.is_empty()
            && (reading.game != HudGame::Aimlab
                || reading.kills.len() as f64 <= AIMLAB_MAX_KILLS_PER_VIDEO_KILL * match_video(tracks).0.len() as f64)
    });
    let Some(reading) = hud else {
        let (flicks, info) = confirmed(tracks, match_video(tracks), checked);
        let all = ClickKills { flicks, info, meta, shots: Vec::new(), hits: None, stats: None };
        let Some(window) = window else {
            return all;
        };
        let mut kill_frames = vec![None; all.info.kills_video];
        for flick in &all.flicks {
            if let Some(slot) = kill_frames.get_mut(flick.kill_number - 1) {
                *slot = Some(flick.kill_frame);
            }
        }
        return kills_within(all, &kill_frames, window);
    };
    let shots = hud_shots(reading);
    // Aim Lab's crosshair is marked as a target by every model: its tracks were taken for the killed target. In
    // KovaaK's runs they stay: a target held under the crosshair looks like them
    let aimlab = reading.game == HudGame::Aimlab;
    let matched = if aimlab { Cow::Owned(without_ghosts(tracks)) } else { Cow::Borrowed(tracks) };
    let times: Vec<f64> = reading.kills.iter().map(|&frame| frame as f64 / tracks.fps).collect();
    let (flicks, mut info) = match_times(&matched, &times, &shots, KILL_WINDOW_S, ON_VIDEO_CLOCK);
    info.source = Some(if aimlab { KillSource::Aimlab } else { KillSource::Hud });
    add_hud_facts(&mut meta, reading, score);
    ClickKills { flicks, info, meta, shots, hits: None, stats: None }
}

/// A tracking run's report, as report.json keeps it: the summary, with the clicking run's parts empty.
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TrackReport {
    /// The recording's name, as the request gave it.
    pub video: String,
    /// The stats file's name; None for a run without one.
    pub stats: Option<String>,
    /// The run's summary (src/tracking.rs): time on target, switches, motion and what-ifs among others.
    pub summary: TrackSummary,
    /// The tracking checks (src/track_checks.rs): a Work on or Fine verdict on each habit the what-ifs measure.
    pub issues: Vec<Issue>,
    /// Always empty: a tracking run has no flicks.
    pub flicks: Vec<Measure>,
    /// Always `Mode::Track`.
    #[cfg_attr(feature = "ts", ts(type = r#""track""#))]
    pub mode: Mode,
    /// Always empty: a tracking run has no kill paths.
    #[cfg_attr(feature = "ts", ts(as = "BTreeMap<String, Vec<crate::typescript::PathPoint>>"))]
    pub paths: BTreeMap<String, Vec<PathPoint>>,
    /// The video's frame rate (frames a second).
    pub fps: f64,
    /// The frame's size, the crosshair's place and the focal length, for the page to turn degrees into pixels.
    pub geometry: Geometry,
    /// Always empty.
    pub appeared: BTreeMap<String, i64>,
    /// Always empty: the crosshair spot is looked for in clicking runs only.
    #[cfg_attr(feature = "ts", ts(as = "Vec<crate::typescript::CrosshairSpot>"))]
    pub crosshair: Capped<(f64, f64), SPOTS>,
    /// The user's run marks as given ({start, end, length}: the service's RunMarks).
    #[cfg_attr(feature = "ts", ts(type = "unknown"))]
    pub run: Option<serde_json::Value>,
    /// The run's length the summary measured (seconds): the run window's, else the stats file's, else the scenario's
    /// time limit; None when none is known.
    pub limit: Option<f64>,
    /// The bots' hitbox its time on target was measured with (None: each target's box), for the page to draw and test
    /// targets the same way.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub hitbox: Option<Hitbox>,
    /// Kept by an older version of the review (`REVIEW_VERSION`): review again for what it lacks.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    #[cfg_attr(feature = "ts", ts(as = "Option<bool>", optional))]
    pub outdated: bool,
}

/// What a tracking run reads from its video besides the tracks: per frame the camera's reading, and whether KovaaK's
/// countdown bar shows.
pub struct VideoReadings<'a> {
    /// Per frame, the camera's reading (src/camera.rs).
    pub camera: &'a [CameraReading],
    /// Per frame, whether KovaaK's countdown bar shows.
    pub countdown: &'a [bool],
}

/// What a tracking run's kill times give: the stats file's facts (or the scenario the file name gives), its name,
/// where the kills come from, the frames bots died on, the challenge's start (a frame, where the kills place it) and
/// the run's length (seconds: the stats file's, else the scenario's time limit).
struct TrackingKills<'a> {
    /// The stats file's "Key:,value" facts by key, or the scenario the file name gives.
    meta: HashMap<String, String>,
    /// The stats file's name; None without one.
    stats: Option<&'a str>,
    /// Where the kills came from: the stats file, the HUD, or the video (which finds none).
    source: KillSource,
    /// The frames bots died on.
    deaths: Vec<i64>,
    /// The frame the challenge starts on, where the stats file's kills place it.
    start: Option<i64>,
    /// The run's length (seconds): the stats file's, else the scenario's time limit.
    limit: Option<f64>,
}

/// A tracking run's kills (bots that die) from its stats file (`name`, `text`); `limit` the scenario's time limit
/// (seconds), which the file's own length overrides.
fn tracking_stats_kills<'a>(
    tracks: &Tracks,
    name: &'a str,
    text: &str,
    limit: Option<f64>,
) -> Result<TrackingKills<'a>, String> {
    let file = StatsFile::parse(text);
    let limit = stats_length(name, &file).or(limit);
    let (mut deaths, mut start) = (Vec::new(), None);
    if !file.rows.is_empty() {
        // bots that die: their kills, matched in the video, and the challenge's start on the video's clock
        let kills = file.kills()?;
        let (flicks, info) = match_times(tracks, &kills.times, &kills.shots, KILL_WINDOW_S, None);
        deaths = flicks.iter().map(|flick| flick.kill_frame).collect();
        if let Some(offset_s) = info.offset
            && info.matched > 0
        {
            start = Some((offset_s * tracks.fps).round_ties_even() as i64);
        }
    }
    Ok(TrackingKills { meta: file.meta, stats: Some(name), source: KillSource::Stats, deaths, start, limit })
}

/// A tracking run's kills without a stats file: KovaaK's HUD's, else none (the video alone finds no deaths).
fn tracking_hud_kills(hud: Option<&HudReading>, video: &str, limit: Option<f64>) -> TrackingKills<'static> {
    let meta = HashMap::from([("Scenario".to_string(), name_parts(video).0)]);
    let (deaths, source) = match hud.filter(|reading| reading.game == HudGame::Kovaak && !reading.kills.is_empty()) {
        Some(reading) => (reading.kills.clone(), KillSource::Hud),
        None => (Vec::new(), KillSource::Video),
    };
    TrackingKills { meta, stats: None, source, deaths, start: None, limit }
}

/// The tracks a tracking run is measured on: without the tracks the user's faint cut-off leaves out, when it is on,
/// and what the cut did.
fn faint_cut(tracks: &Tracks, faint: Option<FaintSetting>) -> (Cow<'_, Tracks>, Option<FaintCut>) {
    let Some(setting) = faint.filter(|setting| setting.on) else {
        return (Cow::Borrowed(tracks), None);
    };
    let without = without_faint(&tracks.frames, setting.offset, TRACKING_FAINT_NEAR_DEG);
    let cut = FaintCut { offset: setting.offset, cut: without.cut, tracks: without.gone };
    (Cow::Owned(Tracks { fps: tracks.fps, frames: without.frames, version: tracks.version }), Some(cut))
}

/// What a tracking review takes from the scenario: its time limit (seconds), which the stats file's own length
/// overrides, and its bots' hitbox (None: the crosshair is on a target within a margin of its box).
#[derive(Clone, Copy, Debug, Default)]
pub struct TrackScenario {
    /// The scenario's time limit (seconds); None when not known.
    pub limit: Option<f64>,
    /// The bots' hitbox; None: each target's box, with a margin.
    pub hitbox: Option<Hitbox>,
}

/// Reviews a tracking run from its tracks, its kill times (bots that die), its scenario's facts and its video's
/// readings. `faint`: the user's faint-target cut-off; when on, the measures leave out the tracks it cuts (the kills
/// are still matched on every track).
pub fn review_tracking(
    tracks: &Tracks,
    kills: KillTimes,
    video: &str,
    scenario: TrackScenario,
    readings: VideoReadings,
    run: Option<serde_json::Value>,
    faint: Option<FaintSetting>,
) -> Result<TrackReport, String> {
    let (fps, limit) = (tracks.fps, scenario.limit);
    let kills = match kills {
        KillTimes::Stats { name, text } => tracking_stats_kills(tracks, name, text, limit)?,
        KillTimes::Unpaired { hud, .. } => tracking_hud_kills(hud, video, limit),
    };
    let TrackingKills { meta, stats, source, deaths, mut start, mut limit } = kills;
    if start.is_none()
        && let Some(length_s) = limit.filter(|&length_s| length_s != 0.0)
    {
        // no kills to place the start: KovaaK's countdown ends it
        let until_s = (tracks.frames.len() as f64 / fps - length_s + COUNTDOWN_SLACK_S).max(MIN_COUNTDOWN_SEARCH_S);
        start = countdown_end(readings.countdown, fps, until_s).map(|frame| frame as i64);
    }
    // the user's own window comes first
    if let Some(marks) = run.as_ref() {
        let (first, length_s) = run_window(marks, fps, limit);
        start = first.or(start);
        limit = length_s;
    }
    let (measured, cut) = faint_cut(tracks, faint);
    let facts = RunFacts {
        meta: &meta,
        limit,
        start,
        camera: Some(readings.camera),
        deaths: &deaths,
        source,
        hitbox: scenario.hitbox,
    };
    let mut summary = track_summary(&measured, &facts);
    summary.faint = cut;
    let issues = crate::track_checks::judge(&summary);
    Ok(TrackReport {
        video: video.into(),
        stats: stats.map(Into::into),
        summary,
        issues,
        flicks: Vec::new(),
        mode: Mode::Track,
        paths: BTreeMap::new(),
        fps,
        geometry: FRAME_GEOMETRY,
        appeared: BTreeMap::new(),
        crosshair: Capped::new(),
        run,
        limit,
        hitbox: scenario.hitbox,
        outdated: false,
    })
}

/// The user's run window (start, end, length in seconds, any of them null) as the first frame and the length in
/// seconds, or (None, limit) where it says nothing (python/retired/review.py: `run_window`). Two of the three settle
/// the third; a start or an end alone takes the length given (the stats file's or the scenario's).
pub fn run_window(marks: &serde_json::Value, fps: f64, limit: Option<f64>) -> (Option<i64>, Option<f64>) {
    let mark = |key: &str| marks.get(key).and_then(serde_json::Value::as_f64);
    let (start_s, end_s) = (mark("start"), mark("end"));
    let frame = |time_s: f64| (time_s * fps).round_ties_even() as i64;
    if let (Some(start_s), Some(end_s)) = (start_s, end_s)
        && end_s > start_s
    {
        return (Some(frame(start_s)), Some(end_s - start_s));
    }
    let given = |length_s: &f64| *length_s != 0.0;
    let length_s = mark("length").filter(given).or(limit.filter(given));
    match (start_s, end_s, length_s) {
        (Some(start_s), _, _) => (Some(frame(start_s)), length_s),
        (None, Some(end_s), Some(given_s)) => (Some(frame((end_s - given_s).max(0.0))), length_s),
        _ => (None, length_s),
    }
}

/// What the service asks the core to review (service/src/report.rs): the tracks, the video's name, the stats file's
/// name and text (empty without one), what the HUD read, the user's run marks; for a clicking run the ammo rules of the
/// scenario's weapon (null or missing: its magazine never runs out, or the scenario is not known); for a tracking run
/// also the scenario's time limit, the video's readings and the user's faint-target cut-off ({on, offset}; null or
/// missing: none).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewRequest {
    /// The run's tracks (tracks.json).
    pub tracks: Tracks,
    /// The recording's name: the report gives it, and without a stats file the scenario and the score come from it.
    pub video: String,
    /// The stats file's name; empty without one.
    #[serde(default)]
    pub stats: String,
    /// The stats file's text; empty without one, and then the kills come from the HUD or the video alone.
    #[serde(default)]
    pub stats_text: String,
    /// What the HUD read (hud.json); None where it was not read.
    #[serde(default)]
    pub hud: Option<HudReading>,
    /// The user's run marks ({start, end, length} in seconds, any of them null); None: no marks.
    #[serde(default)]
    pub run: Option<serde_json::Value>,
    /// Whether the run is a tracking run; else it is reviewed as a clicking run.
    #[serde(default)]
    pub tracking: bool,
    /// A tracking run's scenario time limit (seconds), which the stats file's own length overrides.
    #[serde(default)]
    pub limit: Option<f64>,
    /// A clicking run's ammo rules, when its weapon's magazine runs out.
    #[serde(default)]
    pub reload: Option<AmmoRules>,
    /// A tracking run's camera reading per frame (readings.json).
    #[serde(default)]
    pub camera: Vec<CameraReading>,
    /// A tracking run's countdown bar per frame (readings.json).
    #[serde(default)]
    pub countdown: Vec<bool>,
    /// A tracking run's faint-target cut-off; None: none.
    #[serde(default)]
    pub faint: Option<FaintSetting>,
    /// A tracking run's bot hitbox; None: each target's box.
    #[serde(default)]
    pub hitbox: Option<Hitbox>,
    /// For a run without a stats file, the check of the video's kills in the frames round them (kill_check.rs, kept
    /// as kills.json); None: not checked.
    #[serde(default)]
    pub kill_check: Option<Vec<KillEvidence>>,
}

/// A clicking run's report or a tracking run's.
#[derive(Serialize)]
#[serde(untagged)]
pub enum AnyReport {
    /// A clicking run's report.
    Click(Box<Report>),
    /// A tracking run's report.
    Track(Box<TrackReport>),
}

/// The report, or why there is none: {"report": ...} or {"error": "..."}.
#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    /// The review's report.
    Report(AnyReport),
    /// Why the review failed, in words for the user.
    Error(String),
}

/// Reviews a request: from the stats file when it has one, else from the HUD and the video; as a tracking run or a
/// clicking run as it says. The report is marked outdated when the tracks come from an older review version. Fails
/// when the stats file cannot be read or the summary cannot be made.
fn review_request(request: ReviewRequest) -> Result<AnyReport, String> {
    let kills = if request.stats_text.is_empty() {
        KillTimes::Unpaired { hud: request.hud.as_ref(), checked: request.kill_check.as_deref() }
    } else {
        KillTimes::Stats { name: &request.stats, text: &request.stats_text }
    };
    let outdated = request.tracks.version < REVIEW_VERSION;
    if request.tracking {
        let readings = VideoReadings { camera: &request.camera, countdown: &request.countdown };
        let scenario = TrackScenario { limit: request.limit, hitbox: request.hitbox };
        let (run, faint) = (request.run, request.faint);
        let report = review_tracking(&request.tracks, kills, &request.video, scenario, readings, run, faint)?;
        Ok(AnyReport::Track(Box::new(TrackReport { outdated, ..report })))
    } else {
        let reviewed = review_clicks(&request.tracks, kills, &request.video, request.run, request.reload.as_ref())?;
        Ok(AnyReport::Click(Box::new(Report { outdated, ..reviewed.report })))
    }
}

/// Reviews a request (`ReviewRequest` as JSON) and gives the `Outcome` as JSON: the report, or why there is none
/// (a request that cannot be read is an error too).
pub fn review_json(request: &[u8]) -> Vec<u8> {
    let outcome = serde_json::from_slice::<ReviewRequest>(request)
        .map_err(|error| format!("The review request could not be read: {error}"))
        .and_then(review_request)
        .map_or_else(Outcome::Error, Outcome::Report);
    serde_json::to_vec(&outcome).unwrap_or_default()
}

/// Checks the run window and the kills and HUD counts it keeps.
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// python/retired/review.py's run_window: two marks settle the third; one takes the length given.
    #[test]
    fn the_run_window_as_python_reads_it() {
        let window = |marks: serde_json::Value, limit| run_window(&marks, 60.0, limit);
        assert_eq!(window(json!({"start": 2.0, "end": 12.0, "length": null}), Some(60.0)), (Some(120), Some(10.0)));
        assert_eq!(window(json!({"start": 2.0, "end": null, "length": 5.0}), Some(60.0)), (Some(120), Some(5.0)));
        assert_eq!(window(json!({"start": 2.0, "end": null, "length": null}), Some(60.0)), (Some(120), Some(60.0)));
        assert_eq!(window(json!({"start": null, "end": 12.0, "length": 5.0}), None), (Some(420), Some(5.0)));
        assert_eq!(window(json!({"start": null, "end": 3.0, "length": 5.0}), None), (Some(0), Some(5.0)));
        assert_eq!(window(json!({"start": null, "end": null, "length": null}), Some(60.0)), (None, Some(60.0)));
        assert_eq!(window(json!({"start": 9.0, "end": 3.0, "length": 0.0}), None), (Some(540), None));
    }

    /// A clicking run's window: two marks give its ends, a start alone runs to the last frame or for the length given,
    /// an end alone from the first frame, and a length alone gives none.
    #[test]
    fn a_clicking_runs_window_spans_its_marks() {
        let window = |marks: serde_json::Value| click_window(&marks, 60.0, 1000);
        assert_eq!(window(json!({"start": 2.0, "end": 12.0, "length": null})), Some(120..=720));
        assert_eq!(window(json!({"start": 2.0, "end": null, "length": 5.0})), Some(120..=420));
        assert_eq!(window(json!({"start": 2.0, "end": null, "length": null})), Some(120..=999));
        assert_eq!(window(json!({"start": null, "end": 12.0, "length": null})), Some(0..=720));
        assert_eq!(window(json!({"start": null, "end": null, "length": 5.0})), None);
    }

    /// The HUD's kills, hits and shots outside the window are left out, and its totals counted from the rest.
    #[test]
    fn the_hud_counts_only_inside_the_window() {
        let reading = HudReading {
            game: HudGame::Kovaak,
            kills: vec![10, 50, 100, 150],
            shots: vec![10, 30, 50, 100, 140, 150],
            hits: vec![10, 50, 100, 150],
            totals: HudFinal { kills: 4, hits: Some(4), shots: Some(6) },
            checked: 1.0,
            points: None,
        };
        let inside = hud_within(&reading, &(40..=120));
        assert_eq!((inside.kills, inside.hits, inside.shots), (vec![50, 100], vec![50, 100], vec![50, 100]));
        assert_eq!(inside.totals, HudFinal { kills: 2, hits: Some(2), shots: Some(2) });
    }

    /// A stats file's kills outside the window are left out: the rest numbered again from 1, with their shots and
    /// hits, and the Kills, Hit Count and Miss Count counted from them.
    #[test]
    fn a_stats_file_counts_only_the_kills_inside_the_window() {
        let flick = |kill_number: usize, kill_frame: i64| Flick {
            kill_number,
            kill_frame,
            stats_frame: Some(kill_frame),
            start_frame: kill_frame - 20,
            shots: None,
            path: Vec::new(),
            spawned: false,
            area_px: None,
        };
        let info = MatchInfo {
            kills_video: 4,
            kills_stats: Some(4),
            matched: 3,
            confirmed: None,
            offset: Some(0.0),
            fps: 60.0,
            source: Some(KillSource::Stats),
        };
        let meta = HashMap::from([("Kills".to_string(), "4".to_string()), ("Hit Count".to_string(), "4".to_string())]);
        let kills = ClickKills {
            flicks: vec![flick(2, 50), flick(3, 100), flick(4, 150)],
            info,
            meta,
            shots: vec![1, 3, 2, 1],
            hits: Some(vec![1, 1, 1, 1]),
            stats: Some("run.csv"),
        };
        let kill_frames = [Some(10), Some(50), Some(100), Some(150)];
        let inside = kills_within(kills, &kill_frames, &(40..=120));
        let numbers: Vec<(usize, i64)> =
            inside.flicks.iter().map(|flick| (flick.kill_number, flick.kill_frame)).collect();
        assert_eq!(numbers, vec![(1, 50), (2, 100)]);
        assert_eq!((inside.shots, inside.hits), (vec![3, 2], Some(vec![1, 1])));
        assert_eq!((inside.info.matched, inside.info.kills_stats), (2, Some(2)));
        let count = |key: &str| inside.meta[key].clone();
        assert_eq!((count("Kills"), count("Hit Count"), count("Miss Count")), ("2".into(), "2".into(), "3".into()));
    }
}
