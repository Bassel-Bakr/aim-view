//! A clicking run's summary and its checks (review.py: `summarize`, `_fitts`, `judge`).

use std::collections::HashMap;

use serde::Serialize;

use crate::capped::Capped;
use crate::matching::{KillSource, MatchInfo};
use crate::measure::{flick_profile, Choice, FlickProfile, Measure};
use crate::optional_fields::OptionalFields;
use crate::reload::{ReloadCost, Reloads};
use crate::statistics::{mean, med, median, pstdev};
use crate::what_if::{click_what_if, ClickWhatIf};

/// Click: one shot a kill. Hold: the trigger is held on the target (the median kill takes more than 3 shots).
/// Track: a tracking run, on the target all along.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Click,
    Hold,
    Track,
}

/// A hold-fire run's holding: the median time from reaching a target to its kill, the share of kills where the
/// crosshair slipped off, and the share of that time spent off the target.
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Holding {
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub hold: Option<f64>,
    #[cfg_attr(feature = "ts", ts(as = "Option<f64>", optional))]
    pub slipped: f64,
    #[cfg_attr(feature = "ts", ts(as = "Option<f64>", optional))]
    pub off_share: f64,
}

/// The median kill time in the first and the last third of the run.
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Pace {
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub pace_first: Option<f64>,
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub pace_last: Option<f64>,
}

/// Flicks of one distance range (degrees).
#[derive(Clone, Debug, Default, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct DistanceGroup {
    pub lo: u32,
    pub hi: u32,
    pub n: usize,
    pub interval: Option<f64>,
    pub react: Option<f64>,
    pub short: f64,
    pub past: f64,
    pub still: Option<f64>,
}

/// Flicks of one direction (a 45-degree sector), with the median time each took beyond what its distance predicts.
#[derive(Clone, Debug, Default, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct DirectionGroup {
    #[cfg_attr(feature = "ts", ts(as = "Direction"))]
    pub name: &'static str,
    pub n: usize,
    pub interval: Option<f64>,
    pub distance: Option<f64>,
    pub short: f64,
    pub past: f64,
    pub beyond: Option<f64>,
}

/// A clicking run's summary: the stats file's facts, then the medians and shares of the measures.
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Summary {
    pub scenario: Option<String>,
    pub score: Option<f64>,
    pub kills: i64,
    pub misses: Option<i64>,
    pub fps_avg: Option<f64>,
    pub sens: Option<String>,
    pub fov: Option<String>,
    pub radius: f64,
    pub measured: usize,
    pub info: MatchInfo,
    pub median_interval: Option<f64>,
    pub spread: Option<f64>,
    pub react: Option<f64>,
    pub flick: Option<f64>,
    pub peak: Option<f64>,
    pub arrive: Option<f64>,
    pub still: Option<f64>,
    pub click_speed: Option<f64>,
    pub click_off: Option<f64>,
    pub ended_short: Option<f64>,
    pub ended_past: Option<f64>,
    pub crossed_past: Option<f64>,
    pub moving_clicks: Option<f64>,
    pub shots: Option<i64>,
    pub mode: Mode,
    pub accuracy: Option<f64>,
    #[serde(flatten)]
    pub holding: OptionalFields<Holding>,
    pub still_landed: Option<f64>,
    pub still_corrected: Option<f64>,
    pub short_covered: Option<f64>,
    pub mid_short_cost: Option<f64>,
    #[cfg_attr(feature = "ts", ts(as = "Option<crate::typescript::KillParts>"))]
    pub budget: Option<[f64; 5]>,
    #[cfg_attr(feature = "ts", ts(as = "Vec<DistanceGroup>"))]
    pub by_distance: Capped<DistanceGroup, { DISTANCES.len() }>,
    #[cfg_attr(feature = "ts", ts(as = "Vec<DirectionGroup>"))]
    pub by_direction: Capped<DirectionGroup, { DIRECTIONS.len() }>,
    pub nearest_chosen: Option<f64>,
    pub extra_when_not_nearest: Option<f64>,
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
    Right,
    UpRight,
    Up,
    UpLeft,
    Left,
    DownLeft,
    Down,
    DownRight,
}

/// The distance groups (degrees).
pub const DISTANCES: [(u32, u32); 5] = [(0, 5), (5, 10), (10, 15), (15, 25), (25, 90)];

/// A value of the stats file read as Python's float() reads it.
fn number(meta: &HashMap<String, String>, key: &str) -> Result<Option<f64>, String> {
    meta.get(key)
        .filter(|v| !v.is_empty())
        .map(|v| v.trim().parse().map_err(|_| format!("{key} is not a number: {v}")))
        .transpose()
}

/// A count of the stats file (empty is 0), or None when the file does not give it.
fn count(meta: &HashMap<String, String>, key: &str) -> Result<Option<i64>, String> {
    meta.get(key)
        .map(|v| if v.is_empty() { Ok(0) } else { v.trim().parse().map_err(|_| format!("{key} is not a count: {v}")) })
        .transpose()
}

/// The share of the measures that pass.
fn share(ms: &[&Measure], pass: impl Fn(&Measure) -> bool) -> f64 {
    ms.iter().filter(|m| pass(m)).count() as f64 / ms.len() as f64
}

/// Fitts' law fitted to the run's kills by least squares (t = a + b log2(1 + D / W)): the predicted time of a flick
/// by its distance, or None with too few kills.
fn fitts(ms: &[Measure], w: f64) -> Option<impl Fn(f64) -> f64> {
    let pts: Vec<(f64, f64)> = ms.iter().filter(|m| m.start_distance_deg != 0.0).map(|m| ((1.0 + m.start_distance_deg / w).log2(), m.total)).collect();
    if pts.len() < 3 || w <= 0.0 {
        return None;
    }
    let mx = mean(&pts.iter().map(|p| p.0).collect::<Vec<_>>());
    let my = mean(&pts.iter().map(|p| p.1).collect::<Vec<_>>());
    let sxx: f64 = pts.iter().map(|p| (p.0 - mx) * (p.0 - mx)).sum();
    let b = if sxx != 0.0 { pts.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum::<f64>() / sxx } else { 0.0 };
    let a = my - b * mx;
    Some(move |d: f64| a + b * (1.0 + d / w).log2())
}

pub fn summarize(
    ms: &[Measure],
    ch: &[Choice],
    meta: &HashMap<String, String>,
    info: MatchInfo,
    r: f64,
    mode: Mode,
    reload: Option<&ReloadCost>,
) -> Result<Summary, String> {
    let n = ms.len();
    let all: Vec<&Measure> = ms.iter().collect();
    let video_only = info.source == Some(KillSource::Video); // no stats file: kills from the video, no shots or score
    let of = |pass: &dyn Fn(&Measure) -> bool| (n > 0).then(|| share(&all, pass));
    let hit = number(meta, "Hit Count")?;
    let miss = number(meta, "Miss Count")?;
    let held: Vec<&Measure> = ms.iter().filter(|m| m.hold.is_some_and(|h| h != 0.0)).collect();
    let still_of = |pass: &dyn Fn(&Measure) -> bool| med(ms.iter().filter(|m| pass(m) && m.still.is_some()).map(|m| m.still));
    let short: Vec<f64> = ms.iter().filter(|m| m.end_left > r && m.start_distance_deg > 2.0).map(|m| m.end_left / m.start_distance_deg).collect();
    let mid: Vec<&Measure> = ms.iter().filter(|m| (10.0..25.0).contains(&m.start_distance_deg)).collect();
    let parts: Vec<[f64; 5]> = ms.iter().filter_map(|m| m.parts).collect();
    let fit = fitts(ms, 2.0 * r);
    let totals: Vec<f64> = ms.iter().map(|m| m.total).collect();
    let mut s = Summary {
        scenario: meta.get("Scenario").cloned(),
        score: if video_only || !meta.contains_key("Score") { None } else { Some(number(meta, "Score")?.unwrap_or(0.0)) },
        kills: if video_only { info.matched as i64 } else { count(meta, "Kills")?.unwrap_or(0) },
        misses: count(meta, "Miss Count")?,
        fps_avg: number(meta, "Avg FPS")?,
        sens: meta.get("Horiz Sens").filter(|v| !v.is_empty()).map(|h| {
            format!("{h} {}", meta.get("Sens Scale").map(String::as_str).unwrap_or("None"))
        }),
        fov: meta.get("FOV").cloned(),
        radius: r,
        measured: n,
        info,
        median_interval: med(totals.iter().map(|&t| Some(t))),
        spread: (n > 2).then(|| pstdev(&totals) / mean(&totals)),
        react: med(ms.iter().map(|m| m.react)),
        flick: med(ms.iter().map(|m| m.flick)),
        peak: med(ms.iter().map(|m| Some(m.peak))),
        arrive: med(ms.iter().map(|m| m.arrive)),
        still: med(ms.iter().map(|m| m.still)),
        click_speed: med(ms.iter().map(|m| Some(m.click_speed))),
        click_off: med(ms.iter().map(|m| Some(m.click_off))),
        ended_short: of(&|m| m.end_left > r),
        ended_past: of(&|m| m.end_left < -r),
        crossed_past: of(&|m| m.past > r),
        moving_clicks: of(&|m| m.click_speed > 20.0),
        shots: (!video_only).then(|| ms.iter().map(|m| m.shots.unwrap_or(0)).sum()),
        mode,
        accuracy: match (hit, miss) {
            (Some(h), Some(m)) if h + m > 0.0 => Some(h / (h + m)),
            _ => None,
        },
        holding: OptionalFields((!held.is_empty()).then(|| Holding {
            hold: med(held.iter().map(|m| m.hold)),
            slipped: share(&held, |m| m.breaks > 0),
            off_share: held.iter().map(|m| m.off).sum::<f64>() / held.iter().map(|m| m.hold.unwrap()).sum::<f64>().max(1e-9),
        })),
        still_landed: still_of(&|m| m.end_left <= r),
        still_corrected: still_of(&|m| m.end_left > r),
        short_covered: (!short.is_empty()).then(|| 1.0 - median(&short)),
        mid_short_cost: (!mid.is_empty()).then(|| {
            let landed = |on: bool| med(mid.iter().filter(|m| (m.end_left <= r) == on).map(|m| Some(m.total))).unwrap_or(0.0);
            landed(false) - landed(true)
        }),
        budget: (!parts.is_empty()).then(|| {
            std::array::from_fn(|i| parts.iter().map(|p| p[i]).sum::<f64>() / parts.len() as f64)
        }),
        by_distance: DISTANCES
            .iter()
            .filter_map(|&(lo, hi)| {
                let g: Vec<&Measure> = ms.iter().filter(|m| lo as f64 <= m.start_distance_deg && m.start_distance_deg < hi as f64).collect();
                (!g.is_empty()).then(|| DistanceGroup {
                    lo,
                    hi,
                    n: g.len(),
                    interval: med(g.iter().map(|m| Some(m.total))),
                    react: med(g.iter().map(|m| m.react)),
                    short: share(&g, |m| m.end_left > r),
                    past: share(&g, |m| m.end_left < -r),
                    still: med(g.iter().map(|m| m.still)),
                })
            })
            .collect(),
        by_direction: DIRECTIONS
            .iter()
            .enumerate()
            .filter_map(|(k, &name)| {
                let g: Vec<&Measure> =
                    ms.iter().filter(|m| ((m.direction_deg.rem_euclid(360.0) / 45.0).round_ties_even() as usize) % 8 == k).collect();
                (!g.is_empty()).then(|| DirectionGroup {
                    name,
                    n: g.len(),
                    interval: med(g.iter().map(|m| Some(m.total))),
                    distance: med(g.iter().map(|m| Some(m.start_distance_deg))),
                    short: share(&g, |m| m.end_left > r),
                    past: share(&g, |m| m.end_left < -r),
                    beyond: fit.as_ref().and_then(|f| med(g.iter().map(|m| Some(m.total - f(m.start_distance_deg))))),
                })
            })
            .collect(),
        nearest_chosen: (!ch.is_empty()).then(|| ch.iter().filter(|c| c.rank == 0).count() as f64 / ch.len() as f64),
        extra_when_not_nearest: med(ch.iter().filter(|c| c.rank > 0).map(|c| Some(c.extra))),
        pace: OptionalFields((n >= 30).then(|| Pace {
            pace_first: med(totals[..n / 3].iter().map(|&t| Some(t))),
            pace_last: med(totals[n - n / 3..].iter().map(|&t| Some(t))),
        })),
        what_if: Vec::new(),
        flick_profile: flick_profile(ms),
        reloads: reload.map(|c| c.run.clone()),
    };
    let hits_per_kill = hit.filter(|_| s.kills > 0).map(|h| h / s.kills as f64);
    s.what_if = click_what_if(ms, &s, r, s.info.fps, hits_per_kill, reload);
    Ok(s)
}

/// A check's verdict.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "lowercase")]
pub enum Flag {
    Attention,
    Fine,
}

/// One check: the issue's number (as in docs/issues.md), the number it reads, a plain verdict and why.
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Issue {
    pub issue: u32,
    pub title: &'static str,
    pub value: String,
    pub flag: Flag,
    pub why: String,
}

/// A number with `digits` decimals, as Python's f"{x:.{digits}f}" writes it.
fn fixed(x: f64, digits: usize) -> String {
    format!("{x:.digits$}")
}

/// Seconds as whole milliseconds (review.py: `ms_`).
fn ms(v: Option<f64>) -> String {
    v.map_or("-".into(), |v| format!("{} ms", fixed(1000.0 * v, 0)))
}

/// Python's truth test of a number that may be missing: there and not 0.
fn truthy(v: Option<f64>) -> Option<f64> {
    v.filter(|&v| v != 0.0)
}

/// Provisional checks, one per issue. The thresholds are first guesses until the issue list is settled.
pub fn judge(s: &Summary) -> Vec<Issue> {
    let mut out = Vec::new();
    let mut add = |issue, title, value: String, attention: bool, why: String| {
        let flag = if attention { Flag::Attention } else { Flag::Fine };
        out.push(Issue { issue, title, value, flag, why });
    };
    let hold = s.mode == Mode::Hold;
    if let Some(react) = s.react {
        add(
            1,
            "Slow start",
            format!("{} ms to start moving after a kill (median)", fixed(1000.0 * react, 0)),
            react > 0.15,
            "With several targets on screen this is the switch to the next target, not a reaction to seeing it. \
             Over 150 ms is flagged."
                .into(),
        );
    }
    if let Some(past) = s.ended_past {
        add(
            9,
            "Overflick",
            format!("{}% of flicks overflicked: they ended past the far edge", fixed(100.0 * past, 0)),
            past > 0.15,
            "Over 15% is flagged.".into(),
        );
    }
    if let Some(short) = s.ended_short {
        let cost = s.mid_short_cost;
        let covering = truthy(s.short_covered)
            .map_or(String::new(), |c| format!(", covering {}% of the way", fixed(100.0 * c, 0)));
        add(
            10,
            "Underflick",
            format!("{}% of flicks underflicked{covering}", fixed(100.0 * short, 0)),
            cost.is_some_and(|c| c > 0.03),
            format!(
                "Underflicking is normal; it is flagged only when it costs time: for 10-25 deg flicks the underflicks \
                 took {} against the ones that landed.",
                cost.map_or("an unknown time".into(), |c| format!("{:+.0} ms", 1000.0 * c))
            ),
        );
    }
    if let (true, Some(h)) = (hold, &*s.holding) {
        add(
            24,
            "Unstable landing",
            format!("the crosshair slipped off the target after reaching it in {}% of kills", fixed(100.0 * h.slipped, 0)),
            h.slipped > 0.3,
            "Hold-fire run: each slip breaks the hold and costs time. Over 30% is flagged.".into(),
        );
        add(
            24,
            "Time off the target while holding",
            format!(
                "{}% of the time between reaching a target and killing it was spent off it (median hold {})",
                fixed(100.0 * h.off_share, 0),
                ms(h.hold)
            ),
            h.off_share > 0.15,
            "Over 15% is flagged.".into(),
        );
    }
    if let (false, Some(still), Some(interval)) = (hold, s.still, truthy(s.median_interval)) {
        let share = still / interval;
        let split = match (truthy(s.still_landed), truthy(s.still_corrected)) {
            (Some(l), Some(c)) => format!(
                "When the flick landed on the target, the confirmation took {} ms; after micros, {} ms. ",
                fixed(1000.0 * l, 0),
                fixed(1000.0 * c, 0)
            ),
            _ => String::new(),
        };
        add(
            36,
            "Long confirmation",
            format!(
                "{} ms confirmation: still on the target before the click ({}% of the median TTK)",
                fixed(1000.0 * still, 0),
                fixed(100.0 * share, 0)
            ),
            still > 0.08,
            split + "Over 80 ms is flagged.",
        );
    }
    if let (false, Some(moving)) = (hold, s.moving_clicks) {
        add(
            34,
            "Click on the move",
            format!("{}% of clicks on the move, at over 20 deg/s", fixed(100.0 * moving, 0)),
            moving > 0.10,
            "Over 10% is flagged.".into(),
        );
    }
    if let Some(nearest) = s.nearest_chosen {
        let otherwise = truthy(s.extra_when_not_nearest)
            .map_or(String::new(), |e| format!("; otherwise {} deg farther", fixed(e, 1)));
        add(
            56,
            "Target choice",
            format!("{}% of the time the next target was the nearest{otherwise}", fixed(100.0 * nearest, 0)),
            nearest < 0.6,
            "A farther target can be a planned route, so this is a prompt to check, not a verdict. Under 60% is \
             flagged."
                .into(),
        );
    }
    if let Some(Pace { pace_first: Some(first), pace_last: Some(last) }) = &*s.pace
        && *first != 0.0
        && *last != 0.0
    {
        let drop = last / first - 1.0;
        add(
            49,
            "Pacing drop",
            format!("the TTK was {:+.0}% longer in the last third than in the first", 100.0 * drop),
            drop > 0.10,
            "Over 10% slower is flagged.".into(),
        );
    }
    let dirs: Vec<&DirectionGroup> = s.by_direction.iter().filter(|d| d.n >= 10 && d.beyond.is_some()).collect();
    if let (true, Some(interval)) = (dirs.len() >= 3, truthy(s.median_interval)) {
        let worst = (0..dirs.len()).fold(0, |b, i| if dirs[i].beyond > dirs[b].beyond { i } else { b });
        let others: Vec<f64> =
            dirs.iter().enumerate().filter(|&(i, _)| i != worst).filter_map(|(_, d)| d.beyond).collect();
        let extra = dirs[worst].beyond.unwrap() - median(&others);
        let share = extra / interval;
        add(
            13,
            "Direction bias",
            format!(
                "flicks {} took {} more than the other directions for their distance ({}% of the median TTK)",
                dirs[worst].name,
                ms(Some(extra)),
                fixed(100.0 * share, 0)
            ),
            share > 0.15,
            "Each direction is compared with what its distances predict (Fitts' law fitted to the run), so far and \
             near directions compare fairly. Directions with fewer than 10 flicks are left out. Over 15% of the \
             median TTK is flagged."
                .into(),
        );
    }
    if let (false, Some(misses), true) = (hold, s.misses, s.kills != 0) {
        let rate = misses as f64 / (s.kills + misses).max(1) as f64;
        add(39, "Misses", format!("{misses} misses ({}% of shots)", fixed(100.0 * rate, 1)), rate > 0.08, "Over 8% is flagged.".into());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::fixed;

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
        for (v, w) in want {
            assert_eq!(format!("{}|{}|{:+.0}|{}", fixed(v, 0), fixed(v, 1), v, fixed(v, 2)), w, "{v}");
        }
    }
}
