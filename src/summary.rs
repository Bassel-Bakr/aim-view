//! A clicking run's summary and its checks (the old review, python/retired/review.py: `summarize`, `_fitts`, `judge`).
//!
//! In: the run's measures and target choices (src/measure.rs), its stats file's header, how its kills were matched
//! (src/matching.rs), the target's radius, the run's mode and what reloading cost (src/reload.rs). Out: the report's
//! `Summary`, with its what-if lines (src/what_if.rs), and its checks (`Issue`), which the run page shows.

use std::collections::HashMap;
use std::ops::Range;

use serde::Serialize;

use crate::capped::Capped;
use crate::matching::{KillSource, MatchInfo};
use crate::measure::{Choice, FlickProfile, Measure, flick_profile};
use crate::optional_fields::OptionalFields;
use crate::reload::{ReloadCost, Reloads};
use crate::statistics::{mean, med, median, pstdev};
use crate::what_if::{ClickWhatIf, click_what_if};

/// Each sector of DIRECTIONS spans this many degrees, centered on its direction.
const SECTOR_DEG: f64 = 45.0;
/// Degrees in a full turn: directions are taken modulo it.
const FULL_TURN_DEG: f64 = 360.0;
/// The spread of the TTKs needs this many kills.
const MIN_KILLS_FOR_SPREAD: usize = 3;
/// Fitts' law is fitted to this many kills or more.
const MIN_KILLS_FOR_FIT: usize = 3;
/// The pace compares the first and the last third of a run of this many kills or more.
const MIN_KILLS_FOR_PACE: usize = 30;
/// A click on the move: the crosshair moved faster than this when it clicked (degrees a second).
const MOVING_CLICK_DEG_S: f64 = 20.0;
/// How much of the way an underflick covered counts flicks that started farther than this from the target (degrees).
const MIN_UNDERFLICK_DISTANCE_DEG: f64 = 2.0;
/// What underflicking cost is measured on flicks of this distance (degrees).
const MID_DISTANCE_DEG: Range<f64> = 10.0..25.0;
/// Keeps the share of the held time spent off the target finite when no time was held (seconds).
const MIN_HELD_S: f64 = 1e-9;
/// A share times this is a percentage; seconds times MS_PER_S are milliseconds.
const PERCENT: f64 = 100.0;
/// Milliseconds in a second.
const MS_PER_S: f64 = 1000.0;

/// The checks' thresholds, which their texts give too: first guesses until the issue list is settled. A check is
/// flagged for attention past them (under MIN_NEAREST_SHARE). This one: the median reaction, seconds.
const SLOW_START_S: f64 = 0.15;
/// The share of flicks that overflicked (ended past the target's far edge).
const OVERFLICK_SHARE: f64 = 0.15;
/// What underflicking cost on mid-distance flicks, seconds (`mid_short_cost`).
const UNDERFLICK_COST_S: f64 = 0.03;
/// The share of a hold-fire run's held kills where the crosshair slipped off the target.
const SLIPPED_SHARE: f64 = 0.3;
/// The share of a hold-fire run's held time spent off the target.
const OFF_TARGET_SHARE: f64 = 0.15;
/// The median confirmation (still on the target before the click), seconds.
const LONG_CONFIRMATION_S: f64 = 0.08;
/// The share of clicks on the move (faster than MOVING_CLICK_DEG_S).
const MOVING_CLICKS_SHARE: f64 = 0.10;
/// The share of next targets that were the nearest: flagged under it, not past it.
const MIN_NEAREST_SHARE: f64 = 0.6;
/// How much longer the last third's median TTK is than the first third's, as a share of the first's.
const PACE_DROP_SHARE: f64 = 0.10;
/// The slowest direction's extra time over the others', as a share of the median TTK.
const DIRECTION_BIAS_SHARE: f64 = 0.15;
/// The misses' share of the shots.
const MISS_SHARE: f64 = 0.08;
/// Direction bias compares the directions with this many flicks or more, when there are MIN_BIAS_DIRECTIONS of them.
const MIN_BIAS_FLICKS: usize = 10;
/// The fewest directions with MIN_BIAS_FLICKS flicks for the direction bias check: fewer leave too little to compare.
const MIN_BIAS_DIRECTIONS: usize = 3;

/// Click: one shot a kill. Hold: the trigger is held on the target (the median kill takes more than 3 shots).
/// Track: a tracking run, on the target all along.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// One shot a kill: the confirmation and the clicks on the move are checked.
    Click,
    /// The trigger is held on the target: the holding is checked instead.
    Hold,
    /// A tracking run (src/tracking.rs), on the target all along.
    Track,
}

/// A hold-fire run's holding: the median time from reaching a target to its kill, the share of kills where the
/// crosshair slipped off, and the share of that time spent off the target.
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Holding {
    /// The median hold: seconds from reaching the target to its kill, over the kills held for some time.
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub hold: Option<f64>,
    /// The share of the held kills where the crosshair slipped off the target at least once.
    #[cfg_attr(feature = "ts", ts(as = "Option<f64>", optional))]
    pub slipped: f64,
    /// The share of all the held time spent off the target.
    #[cfg_attr(feature = "ts", ts(as = "Option<f64>", optional))]
    pub off_share: f64,
}

/// The median kill time in the first and the last third of the run.
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Pace {
    /// The median TTK of the first third's kills, seconds.
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub pace_first: Option<f64>,
    /// The median TTK of the last third's kills, seconds.
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub pace_last: Option<f64>,
}

/// Flicks of one distance range (degrees).
#[derive(Clone, Debug, Default, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[expect(clippy::min_ident_chars, reason = "`n` is the JSON's key, which review.py wrote and the run page reads")]
pub struct DistanceGroup {
    /// The group's low end: flicks that started this far from the target or more, degrees.
    pub lo: u32,
    /// The group's high end: flicks that started less than this far, degrees.
    pub hi: u32,
    /// How many flicks are in the group.
    pub n: usize,
    /// Their median TTK, seconds.
    pub interval: Option<f64>,
    /// Their median reaction, seconds.
    pub react: Option<f64>,
    /// The share of them that underflicked (the main flick ended short of the target).
    pub short: f64,
    /// The share of them that overflicked (the main flick ended past the target's far edge).
    pub past: f64,
    /// Their median confirmation, seconds.
    pub still: Option<f64>,
}

/// Flicks of one direction (a 45-degree sector), with the median time each took beyond what its distance predicts.
#[derive(Clone, Debug, Default, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[expect(clippy::min_ident_chars, reason = "`n` is the JSON's key, which review.py wrote and the run page reads")]
pub struct DirectionGroup {
    /// The direction's name in DIRECTIONS ("right", "up-right", ...).
    #[cfg_attr(feature = "ts", ts(as = "Direction"))]
    pub name: &'static str,
    /// How many flicks went this way.
    pub n: usize,
    /// Their median TTK, seconds.
    pub interval: Option<f64>,
    /// Their median distance to the target at the start, degrees.
    pub distance: Option<f64>,
    /// The share of them that underflicked.
    pub short: f64,
    /// The share of them that overflicked.
    pub past: f64,
    /// Their median TTK beyond what Fitts' law, fitted to the run, predicts for their distance (seconds; below 0:
    /// faster); None without a fit.
    pub beyond: Option<f64>,
}

/// A clicking run's summary: the stats file's facts, then the medians and shares of the measures. Without a stats
/// file the facts are those the HUD and the recording's name give (src/review.rs `unpaired_kills`). Times are in
/// seconds, distances in degrees and speeds in degrees a second.
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Summary {
    /// The scenario's name, from the stats file (without one, from the recording's name).
    pub scenario: Option<String>,
    /// The stats file's score; None from the video alone or when the file gives none.
    pub score: Option<f64>,
    /// The stats file's kill count (0 when it gives none); from the video alone, the kills matched.
    pub kills: i64,
    /// The stats file's Miss Count.
    pub misses: Option<i64>,
    /// The game's average frames a second over the run, from the stats file.
    pub fps_avg: Option<f64>,
    /// The sensitivity and its scale as the stats file gives them ("2.5 cm/360"; "None" for a missing scale).
    pub sens: Option<String>,
    /// The field of view as the stats file writes it.
    pub fov: Option<String>,
    /// The targets' radius, degrees (src/measure.rs `target_radius`).
    pub radius: f64,
    /// How many kills were measured: the flicks with a long enough path.
    pub measured: usize,
    /// How the kills were matched to the tracks (src/matching.rs).
    pub info: MatchInfo,
    /// The median TTK, seconds.
    pub median_interval: Option<f64>,
    /// How much the TTKs vary: their standard deviation over their mean, with MIN_KILLS_FOR_SPREAD kills or more.
    pub spread: Option<f64>,
    /// The median reaction, seconds.
    pub react: Option<f64>,
    /// The median main flick, seconds.
    pub flick: Option<f64>,
    /// The median peak speed, degrees a second.
    pub peak: Option<f64>,
    /// The median time to reach the target, seconds.
    pub arrive: Option<f64>,
    /// The median confirmation (still on the target before the click), seconds.
    pub still: Option<f64>,
    /// The median speed at the click, degrees a second.
    pub click_speed: Option<f64>,
    /// The median distance from the target's center at the click, degrees.
    pub click_off: Option<f64>,
    /// The share of flicks that underflicked: the main flick ended short of the target.
    pub ended_short: Option<f64>,
    /// The share of flicks that overflicked: the main flick ended past the target's far edge.
    pub ended_past: Option<f64>,
    /// The share of flicks that went past the target's edge at some point from the reaction's end on.
    pub crossed_past: Option<f64>,
    /// The share of clicks on the move, faster than MOVING_CLICK_DEG_S.
    pub moving_clicks: Option<f64>,
    /// The shots the measured kills took, from the kill times; None from the video alone.
    pub shots: Option<i64>,
    /// Whether the run clicks, holds the trigger or tracks.
    pub mode: Mode,
    /// The stats file's hits over its hits and misses.
    pub accuracy: Option<f64>,
    /// A hold-fire run's holding, over the kills held on the target; its keys are left out without one.
    #[serde(flatten)]
    pub holding: OptionalFields<Holding>,
    /// The median confirmation of the flicks whose main flick did not end short (it landed on the target or past it),
    /// seconds.
    pub still_landed: Option<f64>,
    /// The median confirmation of the flicks whose main flick ended short, so micros brought the crosshair on, seconds.
    pub still_corrected: Option<f64>,
    /// How much of the way an underflick covered (the median share), over flicks that started more than
    /// MIN_UNDERFLICK_DISTANCE_DEG away.
    pub short_covered: Option<f64>,
    /// What underflicking cost: the median TTK of the underflicks in MID_DISTANCE_DEG less that of the other flicks
    /// there, seconds.
    pub mid_short_cost: Option<f64>,
    /// The mean time of each kill step over the kills that have them (react, main flick, onto the target, settle,
    /// still), seconds.
    #[cfg_attr(feature = "ts", ts(as = "Option<crate::typescript::KillParts>"))]
    pub budget: Option<[f64; 5]>,
    /// The flicks of each distance group in DISTANCES that has any.
    #[cfg_attr(feature = "ts", ts(as = "Vec<DistanceGroup>"))]
    pub by_distance: Capped<DistanceGroup, { DISTANCES.len() }>,
    /// The flicks of each direction in DIRECTIONS that has any.
    #[cfg_attr(feature = "ts", ts(as = "Vec<DirectionGroup>"))]
    pub by_direction: Capped<DirectionGroup, { DIRECTIONS.len() }>,
    /// The share of kills after the first whose next target was the nearest on screen (src/measure.rs `choices`).
    pub nearest_chosen: Option<f64>,
    /// When the next target was not the nearest: how much farther it was (the median), degrees.
    pub extra_when_not_nearest: Option<f64>,
    /// The median TTK in the first and the last third, in a run of MIN_KILLS_FOR_PACE kills or more; its keys are left
    /// out in a shorter one.
    #[serde(flatten)]
    pub pace: OptionalFields<Pace>,
    /// What would raise the score, biggest first (src/what_if.rs; Python's report has none).
    pub what_if: Vec<ClickWhatIf>,
    /// The camera's speed through the flicks, averaged (src/measure.rs; Python's report has none).
    pub flick_profile: Option<FlickProfile>,
    /// The reloads an empty magazine forced over the run (src/reload.rs; Python's report has none; none without the
    /// scenario's ammo rules or the kills' shots).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub reloads: Option<Reloads>,
}

/// Eight 45-degree sectors, each centered on its direction (0 = right, 90 = up).
pub const DIRECTIONS: [&str; 8] = ["right", "up-right", "up", "up-left", "left", "down-left", "down", "down-right"];

/// The names in DIRECTIONS, for the TypeScript types only (the core keeps them as strings).
#[cfg(feature = "ts")]
#[derive(ts_rs::TS)]
#[ts(export, rename_all = "kebab-case")]
pub enum Direction {
    /// "right": 0 degrees.
    Right,
    /// "up-right": 45 degrees.
    UpRight,
    /// "up": 90 degrees.
    Up,
    /// "up-left": 135 degrees.
    UpLeft,
    /// "left": 180 degrees.
    Left,
    /// "down-left": 225 degrees.
    DownLeft,
    /// "down": 270 degrees.
    Down,
    /// "down-right": 315 degrees.
    DownRight,
}

/// The distance groups (degrees).
pub const DISTANCES: [(u32, u32); 5] = [(0, 5), (5, 10), (10, 15), (15, 25), (25, 90)];

/// Whether a flick that started `start_distance_deg` from its target is in a distance group of DISTANCES: from the
/// group's low end up to, but not including, its high end.
pub fn in_distance_group(start_distance_deg: f64, (low_deg, high_deg): (u32, u32)) -> bool {
    f64::from(low_deg) <= start_distance_deg && start_distance_deg < f64::from(high_deg)
}

/// The sector of DIRECTIONS a direction is in (degrees: 0 = right, 90 = up).
pub fn direction_sector(direction_deg: f64) -> usize {
    ((direction_deg.rem_euclid(FULL_TURN_DEG) / SECTOR_DEG).round_ties_even() as usize) % DIRECTIONS.len()
}

/// Seconds as whole milliseconds, as Python's f"{1000 * seconds:.0f} ms" writes them.
pub fn whole_ms(seconds: f64) -> String {
    format!("{} ms", ms_number(seconds))
}

/// A value of the stats file read as Python's float() reads it.
fn number(meta: &HashMap<String, String>, key: &str) -> Result<Option<f64>, String> {
    meta.get(key)
        .filter(|value| !value.is_empty())
        .map(|value| value.trim().parse().map_err(|_| format!("{key} is not a number: {value}")))
        .transpose()
}

/// A count of the stats file (empty is 0), or None when the file does not give it.
fn count(meta: &HashMap<String, String>, key: &str) -> Result<Option<i64>, String> {
    let parse = |value: &String| {
        if value.is_empty() {
            return Ok(0);
        }
        value.trim().parse().map_err(|_| format!("{key} is not a count: {value}"))
    };
    meta.get(key).map(parse).transpose()
}

/// The share of the measures that pass.
fn share(measures: &[&Measure], pass: impl Fn(&Measure) -> bool) -> f64 {
    measures.iter().filter(|measure| pass(measure)).count() as f64 / measures.len() as f64
}

/// A flick's index of difficulty in Fitts' law: log2(1 + D / W), its distance over the target's width (degrees).
fn difficulty(distance_deg: f64, width_deg: f64) -> f64 {
    (1.0 + distance_deg / width_deg).log2()
}

/// Fitts' law fitted to the run's kills by least squares (t = a + b log2(1 + D / W), W the target's width in
/// degrees): the predicted TTK of a flick by its distance, or None with too few kills.
fn fitts(measures: &[Measure], width_deg: f64) -> Option<impl Fn(f64) -> f64> {
    let points: Vec<(f64, f64)> = measures
        .iter()
        .filter(|measure| measure.start_distance_deg != 0.0)
        .map(|measure| (difficulty(measure.start_distance_deg, width_deg), measure.total))
        .collect();
    if points.len() < MIN_KILLS_FOR_FIT || width_deg <= 0.0 {
        return None;
    }
    let mean_difficulty = mean(&points.iter().map(|point| point.0).collect::<Vec<_>>());
    let mean_ttk_s = mean(&points.iter().map(|point| point.1).collect::<Vec<_>>());
    let spread: f64 = points.iter().map(|point| (point.0 - mean_difficulty) * (point.0 - mean_difficulty)).sum();
    let slope_s = if spread != 0.0 {
        points.iter().map(|point| (point.0 - mean_difficulty) * (point.1 - mean_ttk_s)).sum::<f64>() / spread
    } else {
        0.0
    };
    let intercept_s = mean_ttk_s - slope_s * mean_difficulty;
    Some(move |distance_deg: f64| intercept_s + slope_s * difficulty(distance_deg, width_deg))
}

/// The score in the stats file (0 when empty), or None from the video alone or when the file gives none.
fn score(meta: &HashMap<String, String>, video_only: bool) -> Result<Option<f64>, String> {
    if video_only || !meta.contains_key("Score") {
        return Ok(None);
    }
    Ok(Some(number(meta, "Score")?.unwrap_or(0.0)))
}

/// The sensitivity and its scale, as the stats file gives them ("None" without a scale).
fn sensitivity(meta: &HashMap<String, String>) -> Option<String> {
    let scale = meta.get("Sens Scale").map(String::as_str).unwrap_or("None");
    meta.get("Horiz Sens").filter(|value| !value.is_empty()).map(|value| format!("{value} {scale}"))
}

/// The hits over the shots, when the stats file gives both and there were any.
fn accuracy(hit_count: Option<f64>, miss_count: Option<f64>) -> Option<f64> {
    match (hit_count, miss_count) {
        (Some(hits), Some(misses)) if hits + misses > 0.0 => Some(hits / (hits + misses)),
        _ => None,
    }
}

/// A hold-fire run's holding, over the kills held on the target for some time.
fn holding(measures: &[Measure]) -> Option<Holding> {
    let held: Vec<&Measure> =
        measures.iter().filter(|measure| measure.hold.is_some_and(|hold_s| hold_s != 0.0)).collect();
    let held_s = || held.iter().filter_map(|measure| measure.hold).sum::<f64>();
    (!held.is_empty()).then(|| Holding {
        hold: med(held.iter().map(|measure| measure.hold)),
        slipped: share(&held, |measure| measure.breaks > 0),
        off_share: held.iter().map(|measure| measure.off).sum::<f64>() / held_s().max(MIN_HELD_S),
    })
}

/// The median confirmation of the kills that pass.
fn confirmation_median(measures: &[Measure], pass: impl Fn(&Measure) -> bool) -> Option<f64> {
    med(measures.iter().filter(|measure| pass(measure)).map(|measure| measure.still))
}

/// How much of the way an underflick covered (the median), over the flicks that started far enough to tell.
fn short_covered(measures: &[Measure], radius_deg: f64) -> Option<f64> {
    let left_shares: Vec<f64> = measures
        .iter()
        .filter(|measure| measure.end_left > radius_deg && measure.start_distance_deg > MIN_UNDERFLICK_DISTANCE_DEG)
        .map(|measure| measure.end_left / measure.start_distance_deg)
        .collect();
    (!left_shares.is_empty()).then(|| 1.0 - median(&left_shares))
}

/// What underflicking cost (seconds): the median TTK of the mid-distance underflicks against the ones that landed.
fn mid_short_cost(measures: &[Measure], radius_deg: f64) -> Option<f64> {
    let mid: Vec<&Measure> =
        measures.iter().filter(|measure| MID_DISTANCE_DEG.contains(&measure.start_distance_deg)).collect();
    let median_ttk_s = |landed: bool| {
        let kills = mid.iter().filter(|measure| (measure.end_left <= radius_deg) == landed);
        med(kills.map(|measure| Some(measure.total))).unwrap_or(0.0)
    };
    (!mid.is_empty()).then(|| median_ttk_s(false) - median_ttk_s(true))
}

/// The mean time of each kill part (react, main flick, onto the target, settle, still), over the kills that have them.
fn budget(measures: &[Measure]) -> Option<[f64; 5]> {
    let parts: Vec<[f64; 5]> = measures.iter().filter_map(|measure| measure.parts).collect();
    let mean_part = |i: usize| parts.iter().map(|kill_parts| kill_parts[i]).sum::<f64>() / parts.len() as f64;
    (!parts.is_empty()).then(|| std::array::from_fn(mean_part))
}

/// The flicks of each distance group that has any.
fn by_distance(measures: &[Measure], radius_deg: f64) -> Capped<DistanceGroup, { DISTANCES.len() }> {
    DISTANCES
        .iter()
        .filter_map(|&(low_deg, high_deg)| {
            let flicks: Vec<&Measure> = measures
                .iter()
                .filter(|measure| in_distance_group(measure.start_distance_deg, (low_deg, high_deg)))
                .collect();
            (!flicks.is_empty()).then(|| DistanceGroup {
                lo: low_deg,
                hi: high_deg,
                n: flicks.len(),
                interval: med(flicks.iter().map(|measure| Some(measure.total))),
                react: med(flicks.iter().map(|measure| measure.react)),
                short: share(&flicks, |measure| measure.end_left > radius_deg),
                past: share(&flicks, |measure| measure.end_left < -radius_deg),
                still: med(flicks.iter().map(|measure| measure.still)),
            })
        })
        .collect()
}

/// The flicks of each direction that has any. `fit`: the TTK Fitts' law predicts for a distance (degrees).
fn by_direction(
    measures: &[Measure],
    radius_deg: f64,
    fit: Option<&impl Fn(f64) -> f64>,
) -> Capped<DirectionGroup, { DIRECTIONS.len() }> {
    DIRECTIONS
        .iter()
        .enumerate()
        .filter_map(|(sector, &name)| {
            let flicks: Vec<&Measure> =
                measures.iter().filter(|measure| direction_sector(measure.direction_deg) == sector).collect();
            (!flicks.is_empty()).then(|| DirectionGroup {
                name,
                n: flicks.len(),
                interval: med(flicks.iter().map(|measure| Some(measure.total))),
                distance: med(flicks.iter().map(|measure| Some(measure.start_distance_deg))),
                short: share(&flicks, |measure| measure.end_left > radius_deg),
                past: share(&flicks, |measure| measure.end_left < -radius_deg),
                beyond: fit.and_then(|predicted_ttk| {
                    med(flicks.iter().map(|measure| Some(measure.total - predicted_ttk(measure.start_distance_deg))))
                }),
            })
        })
        .collect()
}

/// The median TTK in the first and the last third of the run, with MIN_KILLS_FOR_PACE kills or more.
fn pace(ttks_s: &[f64]) -> Option<Pace> {
    let kills = ttks_s.len();
    let third = kills / 3;
    (kills >= MIN_KILLS_FOR_PACE).then(|| Pace {
        pace_first: med(ttks_s[..third].iter().copied().map(Some)),
        pace_last: med(ttks_s[kills - third..].iter().copied().map(Some)),
    })
}

/// A clicking run's summary from its measures and its stats file's header (`meta`); `radius_deg` is the targets'
/// radius. From the video alone (no stats file) the kills are the matched ones, with no shots or score.
pub fn summarize(
    measures: &[Measure],
    choices: &[Choice],
    meta: &HashMap<String, String>,
    info: MatchInfo,
    radius_deg: f64,
    mode: Mode,
    reload: Option<&ReloadCost>,
) -> Result<Summary, String> {
    let video_only = info.source == Some(KillSource::Video);
    let hit_count = number(meta, "Hit Count")?;
    let miss_count = number(meta, "Miss Count")?;
    let measured = measures.len();
    let all: Vec<&Measure> = measures.iter().collect();
    let share_of = |pass: &dyn Fn(&Measure) -> bool| (measured > 0).then(|| share(&all, pass));
    let ttks_s: Vec<f64> = measures.iter().map(|measure| measure.total).collect();
    let fit = fitts(measures, 2.0 * radius_deg);
    let mut summary = Summary {
        scenario: meta.get("Scenario").cloned(),
        score: score(meta, video_only)?,
        kills: if video_only { info.matched as i64 } else { count(meta, "Kills")?.unwrap_or(0) },
        misses: count(meta, "Miss Count")?,
        fps_avg: number(meta, "Avg FPS")?,
        sens: sensitivity(meta),
        fov: meta.get("FOV").cloned(),
        radius: radius_deg,
        measured,
        info,
        median_interval: med(ttks_s.iter().copied().map(Some)),
        spread: (measured >= MIN_KILLS_FOR_SPREAD).then(|| pstdev(&ttks_s) / mean(&ttks_s)),
        react: med(measures.iter().map(|measure| measure.react)),
        flick: med(measures.iter().map(|measure| measure.flick)),
        peak: med(measures.iter().map(|measure| Some(measure.peak))),
        arrive: med(measures.iter().map(|measure| measure.arrive)),
        still: med(measures.iter().map(|measure| measure.still)),
        click_speed: med(measures.iter().map(|measure| Some(measure.click_speed))),
        click_off: med(measures.iter().map(|measure| Some(measure.click_off))),
        ended_short: share_of(&|measure| measure.end_left > radius_deg),
        ended_past: share_of(&|measure| measure.end_left < -radius_deg),
        crossed_past: share_of(&|measure| measure.past > radius_deg),
        moving_clicks: share_of(&|measure| measure.click_speed > MOVING_CLICK_DEG_S),
        shots: (!video_only).then(|| measures.iter().map(|measure| measure.shots.unwrap_or(0)).sum()),
        mode,
        accuracy: accuracy(hit_count, miss_count),
        holding: OptionalFields(holding(measures)),
        still_landed: confirmation_median(measures, |measure| measure.end_left <= radius_deg),
        still_corrected: confirmation_median(measures, |measure| measure.end_left > radius_deg),
        short_covered: short_covered(measures, radius_deg),
        mid_short_cost: mid_short_cost(measures, radius_deg),
        budget: budget(measures),
        by_distance: by_distance(measures, radius_deg),
        by_direction: by_direction(measures, radius_deg, fit.as_ref()),
        nearest_chosen: (!choices.is_empty())
            .then(|| choices.iter().filter(|choice| choice.rank == 0).count() as f64 / choices.len() as f64),
        extra_when_not_nearest: med(choices.iter().filter(|choice| choice.rank > 0).map(|choice| Some(choice.extra))),
        pace: OptionalFields(pace(&ttks_s)),
        what_if: Vec::new(),
        flick_profile: flick_profile(measures),
        reloads: reload.map(|cost| cost.run.clone()),
    };
    let hits_per_kill = hit_count.filter(|_| summary.kills > 0).map(|hits| hits / summary.kills as f64);
    summary.what_if = click_what_if(measures, &summary, radius_deg, summary.info.fps, hits_per_kill, reload);
    Ok(summary)
}

/// A check's verdict.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "lowercase")]
pub enum Flag {
    /// Past the check's threshold: worth a look.
    Attention,
    /// Within the threshold.
    Fine,
}

/// One check: the issue's number, the number it reads, a plain verdict and why. The numbers are those of the old
/// review's issue list (python/retired/review.py cites docs/issues.md, which this repo does not have).
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Issue {
    /// The issue's number in the issue list.
    pub issue: u32,
    /// The check's name, shown as its heading ("Slow start").
    pub title: &'static str,
    /// What the run measured, as a sentence with its number.
    pub value: String,
    /// The verdict: past the threshold or not.
    pub flag: Flag,
    /// Why it matters and the threshold it is flagged at.
    pub why: String,
}

/// A number with `digits` decimals, as Python's f"{x:.{digits}f}" writes it.
fn fixed(x: f64, digits: usize) -> String {
    format!("{x:.digits$}")
}

/// Seconds as a whole number of milliseconds, without the unit.
fn ms_number(seconds: f64) -> String {
    fixed(MS_PER_S * seconds, 0)
}

/// A share as a whole percentage, without the sign.
pub(crate) fn percent_number(share: f64) -> String {
    fixed(PERCENT * share, 0)
}

/// Seconds as whole milliseconds, or "-" when missing (review.py: `ms_`).
fn ms_or_dash(seconds: Option<f64>) -> String {
    seconds.map_or("-".into(), whole_ms)
}

/// Python's truth test of a number that may be missing: there and not 0.
fn truthy(value: Option<f64>) -> Option<f64> {
    value.filter(|&value| value != 0.0)
}

/// One check: flagged for attention, or fine.
pub(crate) fn check(issue: u32, title: &'static str, value: String, attention: bool, why: impl Into<String>) -> Issue {
    let flag = if attention { Flag::Attention } else { Flag::Fine };
    Issue { issue, title, value, flag, why: why.into() }
}

/// The run's checks, each a provisional verdict on one issue (issue 24 has two); a check whose numbers the summary
/// lacks, or that does not apply to the run's mode, is left out. The thresholds are first guesses until the issue list
/// is settled.
pub fn judge(summary: &Summary) -> Vec<Issue> {
    [
        slow_start(summary),
        overflick(summary),
        underflick(summary),
        unstable_landing(summary),
        time_off_while_holding(summary),
        long_confirmation(summary),
        click_on_the_move(summary),
        target_choice(summary),
        pacing_drop(summary),
        direction_bias(summary),
        misses(summary),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// The median reaction after a kill (issue 1); None without one.
fn slow_start(summary: &Summary) -> Option<Issue> {
    let react_s = summary.react?;
    Some(check(
        1,
        "Slow start",
        format!("{} ms to start moving after a kill (median)", ms_number(react_s)),
        react_s > SLOW_START_S,
        "With several targets on screen this is the switch to the next target, not a reaction to seeing it. \
         Over 150 ms is flagged.",
    ))
}

/// The share of flicks that ended past the target's far edge (issue 9); None with no measured flick.
fn overflick(summary: &Summary) -> Option<Issue> {
    let past_share = summary.ended_past?;
    Some(check(
        9,
        "Overflick",
        format!("{}% of flicks overflicked: they ended past the far edge", percent_number(past_share)),
        past_share > OVERFLICK_SHARE,
        "Over 15% is flagged.",
    ))
}

/// The share of flicks that ended short (issue 10), flagged only when it costs time (`mid_short_cost`); None with no
/// measured flick.
fn underflick(summary: &Summary) -> Option<Issue> {
    let short_share = summary.ended_short?;
    let cost_s = summary.mid_short_cost;
    let covering = truthy(summary.short_covered)
        .map_or(String::new(), |covered| format!(", covering {}% of the way", percent_number(covered)));
    let cost = cost_s.map_or("an unknown time".into(), |cost_s| format!("{:+.0} ms", MS_PER_S * cost_s));
    Some(check(
        10,
        "Underflick",
        format!("{}% of flicks underflicked{covering}", percent_number(short_share)),
        cost_s.is_some_and(|cost_s| cost_s > UNDERFLICK_COST_S),
        format!(
            "Underflicking is normal; it is flagged only when it costs time: for 10-25 deg flicks the underflicks \
             took {cost} against the ones that landed."
        ),
    ))
}

/// A hold-fire run's holding, which only a hold-fire run is checked on.
fn hold_fire(summary: &Summary) -> Option<&Holding> {
    if summary.mode != Mode::Hold {
        return None;
    }
    summary.holding.as_ref()
}

/// A hold-fire run's share of held kills where the crosshair slipped off the target (issue 24); None in any other run
/// or without held kills.
fn unstable_landing(summary: &Summary) -> Option<Issue> {
    let holding = hold_fire(summary)?;
    Some(check(
        24,
        "Unstable landing",
        format!(
            "the crosshair slipped off the target after reaching it in {}% of kills",
            percent_number(holding.slipped)
        ),
        holding.slipped > SLIPPED_SHARE,
        "Hold-fire run: each slip breaks the hold and costs time. Over 30% is flagged.",
    ))
}

/// A hold-fire run's share of the held time spent off the target (issue 24 too); None in any other run or without
/// held kills.
fn time_off_while_holding(summary: &Summary) -> Option<Issue> {
    let holding = hold_fire(summary)?;
    Some(check(
        24,
        "Time off the target while holding",
        format!(
            "{}% of the time between reaching a target and killing it was spent off it (median hold {})",
            percent_number(holding.off_share),
            ms_or_dash(holding.hold)
        ),
        holding.off_share > OFF_TARGET_SHARE,
        "Over 15% is flagged.",
    ))
}

/// The confirmation of a run that clicks (issue 36; a hold-fire run has none), against its median TTK.
fn long_confirmation(summary: &Summary) -> Option<Issue> {
    if summary.mode == Mode::Hold {
        return None;
    }
    let (still_s, ttk_s) = (summary.still?, truthy(summary.median_interval)?);
    let split = match (truthy(summary.still_landed), truthy(summary.still_corrected)) {
        (Some(landed_s), Some(corrected_s)) => format!(
            "When the flick landed on the target, the confirmation took {} ms; after micros, {} ms. ",
            ms_number(landed_s),
            ms_number(corrected_s)
        ),
        _ => String::new(),
    };
    Some(check(
        36,
        "Long confirmation",
        format!(
            "{} ms confirmation: still on the target before the click ({}% of the median TTK)",
            ms_number(still_s),
            percent_number(still_s / ttk_s)
        ),
        still_s > LONG_CONFIRMATION_S,
        split + "Over 80 ms is flagged.",
    ))
}

/// The share of clicks on the move (issue 34), in a run that clicks; None in a hold-fire run.
fn click_on_the_move(summary: &Summary) -> Option<Issue> {
    if summary.mode == Mode::Hold {
        return None;
    }
    let moving_share = summary.moving_clicks?;
    Some(check(
        34,
        "Click on the move",
        format!("{}% of clicks on the move, at over 20 deg/s", percent_number(moving_share)),
        moving_share > MOVING_CLICKS_SHARE,
        "Over 10% is flagged.",
    ))
}

/// The share of next targets that were the nearest on screen (issue 56), flagged when it is low; None without choices.
fn target_choice(summary: &Summary) -> Option<Issue> {
    let nearest_share = summary.nearest_chosen?;
    let otherwise = truthy(summary.extra_when_not_nearest)
        .map_or(String::new(), |extra_deg| format!("; otherwise {} deg farther", fixed(extra_deg, 1)));
    Some(check(
        56,
        "Target choice",
        format!("{}% of the time the next target was the nearest{otherwise}", percent_number(nearest_share)),
        nearest_share < MIN_NEAREST_SHARE,
        "A farther target can be a planned route, so this is a prompt to check, not a verdict. Under 60% is \
         flagged.",
    ))
}

/// How much longer the last third's median TTK is than the first's (issue 49); None in a run too short for a pace, or
/// when either median is missing or 0.
fn pacing_drop(summary: &Summary) -> Option<Issue> {
    let &Pace { pace_first: Some(first_s), pace_last: Some(last_s) } = summary.pace.as_ref()? else {
        return None;
    };
    if first_s == 0.0 || last_s == 0.0 {
        return None;
    }
    let drop = last_s / first_s - 1.0;
    Some(check(
        49,
        "Pacing drop",
        format!("the TTK was {:+.0}% longer in the last third than in the first", PERCENT * drop),
        drop > PACE_DROP_SHARE,
        "Over 10% slower is flagged.",
    ))
}

/// The direction that took the longest beyond what its distances predict, against the others, over the directions
/// with enough flicks (issue 13).
fn direction_bias(summary: &Summary) -> Option<Issue> {
    let directions: Vec<&DirectionGroup> = summary
        .by_direction
        .iter()
        .filter(|direction| direction.n >= MIN_BIAS_FLICKS && direction.beyond.is_some())
        .collect();
    let ttk_s = truthy(summary.median_interval)?;
    if directions.len() < MIN_BIAS_DIRECTIONS {
        return None;
    }
    // the first of the slowest, as Python's max() picks it
    let worst = (0..directions.len())
        .fold(0, |slowest, i| if directions[i].beyond > directions[slowest].beyond { i } else { slowest });
    let others_s: Vec<f64> =
        directions.iter().enumerate().filter(|&(i, _)| i != worst).filter_map(|(_, other)| other.beyond).collect();
    let extra_s = directions[worst].beyond? - median(&others_s);
    let share = extra_s / ttk_s;
    Some(check(
        13,
        "Direction bias",
        format!(
            "flicks {} took {} more than the other directions for their distance ({}% of the median TTK)",
            directions[worst].name,
            whole_ms(extra_s),
            percent_number(share)
        ),
        share > DIRECTION_BIAS_SHARE,
        "Each direction is compared with what its distances predict (Fitts' law fitted to the run), so far and \
         near directions compare fairly. Directions with fewer than 10 flicks are left out. Over 15% of the \
         median TTK is flagged.",
    ))
}

/// The misses' share of the shots (issue 39), in a run that clicks and has kills. The shots are the kills and the
/// misses: one hit a kill.
fn misses(summary: &Summary) -> Option<Issue> {
    if summary.mode == Mode::Hold || summary.kills == 0 {
        return None;
    }
    let miss_count = summary.misses?;
    let miss_share = miss_count as f64 / (summary.kills + miss_count).max(1) as f64;
    Some(check(
        39,
        "Misses",
        format!("{miss_count} misses ({}% of shots)", fixed(PERCENT * miss_share, 1)),
        miss_share > MISS_SHARE,
        "Over 8% is flagged.",
    ))
}

/// Tests of the checks' number formatting.
#[cfg(test)]
mod tests {
    use super::fixed;

    /// `fixed` and `{:+.0}` round as Python's f-strings do, ties to even and the sign of -0 kept.
    #[test]
    fn writes_numbers_as_python_does() {
        // Python: f"{v:.0f}|{v:.1f}|{v:+.0f}|{v:.2f}"
        let want = [
            (0.5, "0|0.5|+0|0.50"),
            (1.5, "2|1.5|+2|1.50"),
            (2.5, "2|2.5|+2|2.50"),
            (-0.4, "-0|-0.4|-0|-0.40"),
            (0.125, "0|0.1|+0|0.12"),
            (0.375, "0|0.4|+0|0.38"),
            (2.675, "3|2.7|+3|2.67"),
            (1e-20, "0|0.0|+0|0.00"),
            (-0.0, "-0|-0.0|-0|-0.00"),
        ];
        for (value, written) in want {
            let ours = format!("{}|{}|{:+.0}|{}", fixed(value, 0), fixed(value, 1), value, fixed(value, 2));
            assert_eq!(ours, written, "{value}");
        }
    }
}
