//! What would raise a clicking run's score. Each line is one change, all else the same: the time it would save over
//! the run, turned into kills at the run's own pace (its measured kills over the time they took), since the scenario's
//! time limit stays the same. Each line is a ceiling and they overlap, so they do not add up.

use serde::Serialize;

use crate::capped::Capped;
use crate::measure::Measure;
use crate::reload::ReloadCost;
use crate::statistics::median;
use crate::summary::{Mode, Summary, DIRECTIONS, DISTANCES};

/// The part of the run a line is about.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, rename = "ClickWhatIfGroup"))]
#[serde(rename_all = "lowercase")]
pub enum Group {
    Pace,
    Flicks,
    Micros,
}

/// One line: the extra kills over the run, the extra score (null when the run gives no score per kill), and what was
/// assumed.
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ClickWhatIf {
    pub group: Group,
    pub what: &'static str,
    pub kills: f64,
    pub score: Option<f64>,
    pub how: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Line {
    Reaction,
    Confirmation,
    BestTen,
    NextTarget,
    Flick,
    Land,
    Direction,
    FewerMicros,
    SmallerMicros,
    Miss,
    Slip,
    Reload,
}

/// The numbers (and the direction's name) a line's sentence gives.
#[derive(Default)]
struct Facts {
    a: f64,
    b: f64,
    name: &'static str,
    /// The points hitting every shot would add on its own, in a scenario whose score is scaled by accuracy; or the
    /// points the reloads saved would give back, in a scenario that takes points off for a reload.
    points: Option<f64>,
}

/// One line's saving: seconds over the run.
struct Saving {
    line: Line,
    seconds: f64,
    facts: Facts,
}

/// Every line's words, in one place.
fn words(line: Line, f: &Facts) -> (Group, &'static str, String) {
    let ms = |s: f64| format!("{:.0} ms", 1000.0 * s);
    match line {
        Line::Reaction => (
            Group::Pace,
            "React faster",
            format!("Each reaction cut to {}, the edge of your fastest quarter.", ms(f.a)),
        ),
        Line::Confirmation => (
            Group::Pace,
            "Shorter confirmation",
            format!("Each confirmation before the click cut to your median, {}.", ms(f.a)),
        ),
        Line::BestTen => (
            Group::Pace,
            "Keep up your best 10 seconds all run",
            format!("Your best 10 seconds went at {:.0} kills a minute, against {:.0} over the run.", f.a, f.b),
        ),
        Line::NextTarget => (
            Group::Pace,
            "Get to the next target faster",
            format!("The time from each kill until on the next target cut to {}, the edge of your fastest quarter.", ms(f.a)),
        ),
        Line::Flick => (
            Group::Flicks,
            "Flick faster",
            "Each flick at the speed of your fastest quarter of flicks of its distance.".into(),
        ),
        Line::Land => (
            Group::Flicks,
            "Fewer overflicks and underflicks",
            format!("The time from the flick's end until on the target, after your {:.0} overflicks and underflicks.", f.a),
        ),
        Line::Direction => (
            Group::Flicks,
            "Flick every direction like your best one",
            format!(
                "Each direction's flicks at the speed of your best direction ({}) for their distance, among directions \
                 with 5% of flicks or more.",
                f.name
            ),
        ),
        Line::FewerMicros => (
            Group::Micros,
            "Fewer micros",
            format!("Kills with more micros than your median ({}) brought to the median micro time of the others, {}.", f.a, ms(f.b)),
        ),
        Line::SmallerMicros => (
            Group::Micros,
            "Smaller micros",
            format!("The micro time after your {:.0} flicks that landed on the target but off its center.", f.a),
        ),
        Line::Miss => (
            Group::Micros,
            "Don't miss",
            match f.points {
                Some(p) => format!(
                    "Each kill with a miss brought to the TTK of your clean kills of its distance, about {} a miss; \
                     your score is also scaled by your accuracy, so hitting every shot would add about {p:.0} points \
                     on its own.",
                    ms(f.a)
                ),
                None => format!(
                    "Each kill with a miss brought to the TTK of your clean kills of its distance, about {} a miss.",
                    ms(f.a)
                ),
            },
        ),
        Line::Reload => {
            let reloads = if f.a == 1.0 { "reload" } else { "reloads" };
            let given = format!(
                "Your misses forced {:.0} {reloads} of an empty magazine, {:.1} s in all. Reloads you chose yourself \
                 don't show in the stats and are not counted.",
                f.a, f.b
            );
            let how = match f.points {
                Some(p) => format!("{given} Each reload also takes points off: about {p:.0} points back on their own."),
                None => given,
            };
            (Group::Pace, "Reload less: miss less", how)
        }
        Line::Slip => (
            Group::Micros,
            "Don't slip off the target",
            "The time off the target after first reaching it, while firing.".into(),
        ),
    }
}

/// The q-quantile with linear interpolation between the sorted values (numpy's default).
fn quantile(values: &[f64], q: f64) -> f64 {
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    let pos = q * (v.len() - 1) as f64;
    let (lo, frac) = (pos.floor() as usize, pos.fract());
    if lo + 1 < v.len() { v[lo] + frac * (v[lo + 1] - v[lo]) } else { v[lo] }
}

/// The time above `cap`, summed.
fn above(values: &[f64], cap: f64) -> f64 {
    values.iter().map(|&x| (x - cap).max(0.0)).sum()
}

/// Each value cut to the edge of its fastest quarter (the 25th percentile), with 4 values or more.
fn cut_to_quarter(line: Line, values: Vec<f64>) -> Option<Saving> {
    (values.len() >= 4).then(|| {
        let q = quantile(&values, 0.25);
        Saving { line, seconds: above(&values, q), facts: Facts { a: q, ..Facts::default() } }
    })
}

fn reaction(ms: &[Measure]) -> Option<Saving> {
    cut_to_quarter(Line::Reaction, ms.iter().filter_map(|m| m.react).collect())
}

fn next_target(ms: &[Measure]) -> Option<Saving> {
    cut_to_quarter(Line::NextTarget, ms.iter().filter_map(|m| m.arrive).collect())
}

fn confirmation(ms: &[Measure]) -> Option<Saving> {
    let still: Vec<f64> = ms.iter().filter_map(|m| m.still).collect();
    (still.len() >= 4).then(|| {
        let m = median(&still);
        Saving { line: Line::Confirmation, seconds: above(&still, m), facts: Facts { a: m, ..Facts::default() } }
    })
}

/// The best 10 s window's kill rate kept all run: the measured kills would take `kills / best` instead of `total`.
/// The windows start at the run's first flick or at a kill and end by the last kill; the run must span 20 s.
fn best_ten(ms: &[Measure], fps: f64, rate: f64, total: f64) -> Option<Saving> {
    let mut t: Vec<f64> = ms.iter().map(|m| m.kill_frame as f64 / fps).collect();
    t.sort_by(f64::total_cmp);
    let first = ms.iter().map(|m| m.start_frame).min()? as f64 / fps;
    let last = *t.last()?;
    if last - first < 20.0 {
        return None;
    }
    let best = std::iter::once(first)
        .chain(t.iter().copied())
        .filter(|&s| s + 10.0 <= last)
        .map(|s| t.iter().filter(|&&k| s < k && k <= s + 10.0).count())
        .max()? as f64
        / 10.0;
    (best > 0.0).then(|| Saving {
        line: Line::BestTen,
        seconds: (total - ms.len() as f64 / best).max(0.0),
        facts: Facts { a: 60.0 * best, b: 60.0 * rate, ..Facts::default() },
    })
}

/// A flick's way covered and time: from the start to where the flick ended (its speed is the one over the other).
fn covered(m: &Measure) -> Option<(f64, f64)> {
    let w = m.d0 - m.end_left;
    m.flick.filter(|&f| f > 0.0 && w > 0.0).map(|f| (w, f))
}

/// The measures in each distance group (summary.rs: `DISTANCES`) that have a flick speed.
fn by_distance(ms: &[Measure]) -> [Vec<(f64, f64)>; DISTANCES.len()] {
    DISTANCES.map(|(lo, hi)| ms.iter().filter(|m| lo as f64 <= m.d0 && m.d0 < hi as f64).filter_map(covered).collect())
}

/// Each flick at the speed of the fastest quarter (the 75th percentile) of its distance group's flicks.
fn flick_faster(ms: &[Measure]) -> Option<Saving> {
    let saved: Vec<f64> = by_distance(ms)
        .into_iter()
        .filter(|g| g.len() >= 4)
        .map(|g| {
            let v = quantile(&g.iter().map(|&(w, f)| w / f).collect::<Vec<_>>(), 0.75);
            g.iter().map(|&(w, f)| (f - w / v).max(0.0)).sum()
        })
        .collect();
    (!saved.is_empty()).then(|| Saving { line: Line::Flick, seconds: saved.iter().sum(), facts: Facts::default() })
}

/// After a flick that ended short of or past the target, the time from its end until on the target.
fn land(ms: &[Measure], r: f64) -> Option<Saving> {
    let t: Vec<f64> = ms.iter().filter(|m| m.end_left.abs() > r).filter_map(|m| m.parts.map(|p| p[2])).collect();
    (!t.is_empty()).then(|| Saving {
        line: Line::Land,
        seconds: t.iter().sum(),
        facts: Facts { a: t.len() as f64, ..Facts::default() },
    })
}

/// Each direction's flicks at its best direction's speed. A flick's speed is taken against its distance group's median
/// (groups of 3 or more), so near and far directions compare fairly; directions need 5% of the flicks and 3 or more.
fn direction(ms: &[Measure]) -> Option<Saving> {
    let mut groups: [Vec<(f64, f64)>; DIRECTIONS.len()] = Default::default();
    for &(lo, hi) in DISTANCES.iter() {
        let g: Vec<(&Measure, (f64, f64))> = ms
            .iter()
            .filter(|m| lo as f64 <= m.d0 && m.d0 < hi as f64)
            .filter_map(|m| covered(m).map(|c| (m, c)))
            .collect();
        if g.len() < 3 {
            continue;
        }
        let mid = median(&g.iter().map(|&(_, (w, f))| w / f).collect::<Vec<_>>());
        for (m, (w, f)) in g {
            let k = ((m.dir.rem_euclid(360.0) / 45.0).round_ties_even() as usize) % 8;
            groups[k].push((w / f / mid, f));
        }
    }
    let n: usize = groups.iter().map(Vec::len).sum();
    let need = (0.05 * n as f64).ceil().max(3.0) as usize;
    let speeds: Capped<(usize, f64), { DIRECTIONS.len() }> = (0..groups.len())
        .filter(|&k| groups[k].len() >= need)
        .map(|k| (k, median(&groups[k].iter().map(|p| p.0).collect::<Vec<_>>())))
        .collect();
    if speeds.len() < 2 {
        return None;
    }
    let &(best_k, best) = speeds.iter().max_by(|a, b| a.1.total_cmp(&b.1))?;
    let seconds = speeds.iter().map(|&(k, s)| groups[k].iter().map(|p| p.1 * (1.0 - s / best)).sum::<f64>()).sum();
    Some(Saving { line: Line::Direction, seconds, facts: Facts { name: DIRECTIONS[best_k], ..Facts::default() } })
}

/// The kills with more than the median count of micros, their settle time cut to the median settle time of the
/// others.
fn fewer_micros(ms: &[Measure]) -> Option<Saving> {
    let g: Vec<(f64, f64)> = ms.iter().filter_map(|m| m.settle.map(|s| (m.corr as f64, s))).collect();
    if g.len() < 4 {
        return None;
    }
    let c = median(&g.iter().map(|p| p.0).collect::<Vec<_>>());
    let few: Vec<f64> = g.iter().filter(|p| p.0 <= c).map(|p| p.1).collect();
    let cap = median(&few);
    let seconds = g.iter().filter(|p| p.0 > c).map(|p| (p.1 - cap).max(0.0)).sum();
    Some(Saving { line: Line::FewerMicros, seconds, facts: Facts { a: c, b: cap, ..Facts::default() } })
}

/// The settle time after flicks that ended on the target (neither short nor past) and were still corrected.
fn smaller_micros(ms: &[Measure], r: f64) -> Option<Saving> {
    let t: Vec<f64> = ms.iter().filter(|m| m.end_left.abs() <= r && m.corr > 0).filter_map(|m| m.settle).collect();
    (!t.is_empty()).then(|| Saving {
        line: Line::SmallerMicros,
        seconds: t.iter().sum(),
        facts: Facts { a: t.len() as f64, ..Facts::default() },
    })
}

/// Each kill that took more shots than a kill needs (the stats file's hits per kill, else the fewest shots of a kill),
/// brought to the median TTK of the clean kills of its distance group (3 or more of them).
fn miss(ms: &[Measure], hits_per_kill: Option<f64>, points: Option<f64>) -> Option<Saving> {
    let base = match hits_per_kill {
        Some(h) => (h.round() as i64).max(1),
        None => ms.iter().filter_map(|m| m.shots).filter(|&s| s >= 1).min()?,
    };
    let (mut seconds, mut misses) = (0.0, 0);
    for &(lo, hi) in DISTANCES.iter() {
        let g: Vec<&Measure> = ms.iter().filter(|m| lo as f64 <= m.d0 && m.d0 < hi as f64).collect();
        let clean: Vec<f64> = g.iter().filter(|m| m.shots == Some(base)).map(|m| m.total).collect();
        if clean.len() < 3 {
            continue;
        }
        let cap = median(&clean);
        for m in g.iter().filter(|m| m.shots.is_some_and(|s| s > base)) {
            seconds += (m.total - cap).max(0.0);
            misses += m.shots.unwrap() - base;
        }
    }
    (misses > 0).then(|| Saving {
        line: Line::Miss,
        seconds,
        facts: Facts { a: seconds / misses as f64, points, ..Facts::default() },
    })
}

/// In a hold-fire run, the time off the target after first reaching it.
fn slip(ms: &[Measure]) -> Option<Saving> {
    let t: Vec<f64> = ms.iter().filter(|m| m.hold.is_some()).map(|m| m.off).collect();
    (!t.is_empty()).then(|| Saving { line: Line::Slip, seconds: t.iter().sum(), facts: Facts::default() })
}

/// The reload time the misses forced: the run's forced reloads against the same run with each kill's shots cut to its
/// hits (src/reload.rs).
fn reload(c: &ReloadCost) -> Option<Saving> {
    let clean = c.clean.as_ref()?;
    let saved = c.run.count - clean.count;
    (saved > 0).then(|| Saving {
        line: Line::Reload,
        seconds: c.run.seconds - clean.seconds,
        facts: Facts {
            a: saved as f64,
            b: c.run.seconds - clean.seconds,
            points: c.run.score_lost.map(|p| p - clean.score_lost.unwrap_or(0.0)),
            ..Facts::default()
        },
    })
}

/// The points hitting every shot would add, when the stats file shows the score is the kills times whole points
/// times the accuracy (and not whole points a kill).
fn accuracy_points(s: &Summary) -> Option<f64> {
    let (score, acc, kills) = (s.score?, s.accuracy?, s.kills as f64);
    if kills <= 0.0 || acc <= 0.0 || acc >= 1.0 {
        return None;
    }
    let whole = |x: f64| (x - x.round()).abs() < 1e-3 * x.abs().max(1.0);
    (whole(score / (kills * acc)) && !whole(score / kills)).then(|| score / acc - score)
}

/// The run's what-if lines, biggest first, without those under half a kill. A hold-fire run has no click to wait for
/// or correct before: it gets the time to the next target and the time off it while firing instead of the
/// confirmation, the micros and the misses. `reload`: what reloading cost the run, when its weapon's magazine runs out.
pub fn click_what_if(
    ms: &[Measure],
    s: &Summary,
    r: f64,
    fps: f64,
    hits_per_kill: Option<f64>,
    reload: Option<&ReloadCost>,
) -> Vec<ClickWhatIf> {
    let total: f64 = ms.iter().map(|m| m.total).sum();
    if ms.is_empty() || total <= 0.0 {
        return Vec::new();
    }
    let rate = ms.len() as f64 / total;
    let per_kill = s.score.filter(|_| s.kills > 0).map(|v| v / s.kills as f64);
    let (hold, click) = (s.mode == Mode::Hold, s.mode != Mode::Hold);
    let lines = [
        reaction(ms),
        click.then(|| confirmation(ms)).flatten(),
        best_ten(ms, fps, rate, total),
        hold.then(|| next_target(ms)).flatten(),
        flick_faster(ms),
        land(ms, r),
        direction(ms),
        click.then(|| fewer_micros(ms)).flatten(),
        click.then(|| smaller_micros(ms, r)).flatten(),
        click.then(|| miss(ms, hits_per_kill, accuracy_points(s))).flatten(),
        hold.then(|| slip(ms)).flatten(),
        reload.and_then(self::reload),
    ];
    let mut out: Vec<ClickWhatIf> = lines
        .into_iter()
        .flatten()
        .filter_map(|v| {
            let kills = v.seconds.min(total) * rate;
            (kills >= 0.5).then(|| {
                let (group, what, how) = words(v.line, &v.facts);
                ClickWhatIf { group, what, kills, score: per_kill.map(|p| kills * p), how }
            })
        })
        .collect();
    out.sort_by(|a, b| b.kills.total_cmp(&a.kills));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A made-up flick: distance, direction, kill time, reaction, flick time, where the flick ended, micros, settle,
    /// confirmation, shots.
    #[allow(clippy::too_many_arguments)]
    fn flick(d0: f64, dir: f64, total: f64, react: f64, fl: f64, end_left: f64, corr: usize, settle: f64, still: f64, shots: i64) -> Measure {
        Measure {
            n: 0,
            shots: Some(shots),
            d0,
            dir,
            total,
            react: Some(react),
            flick: Some(fl),
            peak: 0.0,
            end_left,
            end_off: end_left.abs(),
            arrive: Some(react + fl + 0.05),
            dwell: None,
            past: 0.0,
            corr,
            click_speed: 0.0,
            click_off: 0.0,
            click_off_xy: (0.0, 0.0),
            settle: Some(settle),
            still: Some(still),
            start_frame: 0,
            kill_frame: 0,
            spawned: false,
            hold: Some(settle + still),
            breaks: 0,
            off: 0.02,
            parts: Some([react, fl, 0.05, settle, still]),
            speed: None,
            reloads: None,
            reload_time: None,
        }
    }

    /// Four 6-degree flicks right, four left (0 = right, 180 = left): r = 0.5.
    fn run() -> Vec<Measure> {
        vec![
            flick(6.0, 0.0, 0.40, 0.10, 0.10, 0.0, 0, 0.00, 0.05, 1),
            flick(6.0, 0.0, 0.50, 0.12, 0.12, 1.0, 1, 0.04, 0.06, 1),
            flick(6.0, 0.0, 0.60, 0.14, 0.15, -1.0, 2, 0.08, 0.07, 2),
            flick(6.0, 0.0, 0.70, 0.20, 0.20, 0.2, 3, 0.12, 0.10, 1),
            flick(6.0, 180.0, 0.40, 0.10, 0.12, 0.0, 0, 0.02, 0.05, 1),
            flick(6.0, 180.0, 0.50, 0.12, 0.15, 0.0, 1, 0.04, 0.06, 1),
            flick(6.0, 180.0, 0.60, 0.14, 0.20, 0.0, 1, 0.06, 0.08, 3),
            flick(6.0, 180.0, 0.80, 0.30, 0.24, 0.0, 2, 0.10, 0.12, 1),
        ]
    }

    fn close(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "{a} against {b}");
    }

    #[test]
    fn quantiles_as_numpy_gives_them() {
        close(quantile(&[4.0, 1.0, 3.0, 2.0], 0.25), 1.75);
        close(quantile(&[4.0, 1.0, 3.0, 2.0], 0.75), 3.25);
        close(quantile(&[5.0], 0.25), 5.0);
    }

    #[test]
    fn pace_lines() {
        let ms = run();
        // reactions 0.10 0.10 0.12 0.12 0.14 0.14 0.20 0.30: the 25th percentile is 0.115
        let s = reaction(&ms).unwrap();
        close(s.facts.a, 0.115);
        close(s.seconds, 0.005 + 0.005 + 0.025 + 0.025 + 0.085 + 0.185);
        // confirmations 0.05 0.05 0.06 0.06 0.07 0.08 0.10 0.12: the median is 0.065
        let s = confirmation(&ms).unwrap();
        close(s.seconds, 0.005 + 0.015 + 0.035 + 0.055);
        // onto the target: reaction + flick + 0.05
        let s = next_target(&ms).unwrap();
        let arrive: Vec<f64> = ms.iter().map(|m| m.arrive.unwrap()).collect();
        close(s.seconds, above(&arrive, quantile(&arrive, 0.25)));
    }

    #[test]
    fn the_best_ten_seconds() {
        // 30 kills a second apart, then 10 kills 3 s apart: 10 kills in the best 10 s
        let mut ms = Vec::new();
        let mut t = 0.0;
        for i in 0..40 {
            t += if i < 30 { 1.0 } else { 3.0 };
            let mut m = flick(6.0, 0.0, if i < 30 { 1.0 } else { 3.0 }, 0.1, 0.1, 0.0, 0, 0.0, 0.0, 1);
            m.start_frame = ((t - m.total) * 60.0) as i64;
            m.kill_frame = (t * 60.0) as i64;
            ms.push(m);
        }
        let total = 60.0;
        let s = best_ten(&ms, 60.0, 40.0 / total, total).unwrap();
        // at 1 kill a second the 40 kills take 40 s: 20 s saved
        close(s.facts.a, 60.0);
        close(s.seconds, 20.0);
    }

    #[test]
    fn flick_lines() {
        let ms = run();
        // speeds (way / time), right: 60, 41.67, 46.67, 29; left: 50, 40, 30, 25. All in the 5-10 degree group.
        let speeds = [60.0, 5.0 / 0.12, 7.0 / 0.15, 5.8 / 0.2, 6.0 / 0.12, 6.0 / 0.15, 6.0 / 0.2, 6.0 / 0.24];
        let v = quantile(&speeds, 0.75);
        let want: f64 = ms.iter().map(|m| covered(m).map_or(0.0, |(w, f)| (f - w / v).max(0.0))).sum();
        close(flick_faster(&ms).unwrap().seconds, want);
        // two flicks ended past r = 0.5 (one short, one past): their 0.05 s onto the target
        let s = land(&ms, 0.5).unwrap();
        close(s.seconds, 0.10);
        close(s.facts.a, 2.0);
        // right is the faster direction: left's flicks take its median speed ratio
        let mid = median(&speeds);
        let right = median(&speeds[..4].iter().map(|v| v / mid).collect::<Vec<_>>());
        let left = median(&speeds[4..].iter().map(|v| v / mid).collect::<Vec<_>>());
        let s = direction(&ms).unwrap();
        assert_eq!(s.facts.name, "right");
        close(s.seconds, (0.12 + 0.15 + 0.20 + 0.24) * (1.0 - left / right));
    }

    #[test]
    fn micro_lines() {
        let ms = run();
        // micros 0 1 2 3 0 1 1 2: the median is 1; settle of those with 1 or fewer: 0, 0.04, 0.02, 0.04, 0.06
        let s = fewer_micros(&ms).unwrap();
        close(s.facts.b, 0.04);
        close(s.seconds, (0.08 - 0.04) + (0.12 - 0.04) + (0.10 - 0.04));
        // landed (|end_left| <= 0.5) with micros: 0.12 + 0.04 + 0.06 + 0.10
        close(smaller_micros(&ms, 0.5).unwrap().seconds, 0.32);
        // one hit a kill; clean kills' median TTK 0.5; the 2-shot kill (0.6) and the 3-shot kill (0.6): 3 misses
        let s = miss(&ms, Some(1.0), None).unwrap();
        close(s.seconds, 0.2);
        close(s.facts.a, 0.2 / 3.0);
        close(slip(&ms).unwrap().seconds, 0.16);
    }

    #[test]
    fn reload_line() {
        use crate::reload::Reloads;
        // 3 forced reloads, 1 without the misses: 2 saved, a second, and 50 of the 75 points they took off
        let run = Reloads { count: 3, seconds: 1.5, score_lost: Some(75.0) };
        let clean = Some(Reloads { count: 1, seconds: 0.5, score_lost: Some(25.0) });
        let s = reload(&ReloadCost { per_kill: Vec::new(), run, clean }).unwrap();
        close(s.seconds, 1.0);
        close(s.facts.a, 2.0);
        assert_eq!(s.facts.points, Some(50.0));
        assert!(words(s.line, &s.facts).2.contains("2 reloads of an empty magazine, 1.0 s"));
    }
}
