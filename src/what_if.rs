//! What would raise a clicking run's score. Each line is one change, all else the same: the time it would save over
//! the run, turned into kills at the run's own pace (its measured kills over the time they took), since the scenario's
//! time limit stays the same. Each line is a ceiling and they overlap, so they do not add up.
//!
//! In: the run's measures (src/measure.rs), its summary (src/summary.rs: the mode, the kills, the score and the
//! accuracy), the target's radius, the frame rate, the hits a kill takes and what reloading cost (src/reload.rs). Out:
//! the summary's `what_if` lines, biggest first, which the run page's report lists.

use serde::Serialize;

use crate::capped::Capped;
use crate::measure::Measure;
use crate::reload::ReloadCost;
use crate::statistics::median;
use crate::summary::{DIRECTIONS, DISTANCES, Mode, Summary, direction_sector, in_distance_group, whole_ms};

/// A line that cuts times to their median or to a quantile of them needs this many of them.
const MIN_TIMES: usize = 4;
/// A distance group's median flick speed, or its clean kills' median TTK, needs this many of them; a direction needs
/// this many flicks too.
const MIN_IN_GROUP: usize = 3;
/// The edge of the fastest quarter of times: this quantile of them (the 25th percentile).
const FASTEST_QUARTER_OF_TIMES: f64 = 0.25;
/// The edge of the fastest quarter of speeds: this quantile of them (the 75th percentile).
const FASTEST_QUARTER_OF_SPEEDS: f64 = 0.75;
/// "Keep up your best 10 seconds": the window, in seconds.
const BEST_WINDOW_S: f64 = 10.0;
/// How long the run must span, from its first flick to its last kill, for the best-window line, in seconds.
const MIN_RUN_FOR_BEST_S: f64 = 20.0;
/// Seconds in a minute, for the kills a minute the line's sentence gives.
const SECONDS_PER_MINUTE: f64 = 60.0;
/// A direction counts when it holds this share of the flicks (and MIN_IN_GROUP or more).
const MIN_DIRECTION_SHARE: f64 = 0.05;
/// The direction line needs its best direction and another one to bring to it.
const MIN_DIRECTIONS: usize = 2;
/// A line shows when it would add this many kills or more.
const MIN_KILLS_SHOWN: f64 = 0.5;
/// A number is whole when it is this share of itself (of 1, for a number under 1) or nearer to a whole number.
const WHOLE_TOLERANCE: f64 = 1e-3;
/// The time onto the target, in a measure's `parts` (react, main flick, onto the target, settle, still).
const ONTO_TARGET_PART: usize = 2;

/// The part of the run a line is about.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, rename = "ClickWhatIfGroup"))]
#[serde(rename_all = "lowercase")]
pub enum Group {
    /// The time between kills: reaction, confirmation, the run's best stretch, the next target, reloads.
    Pace,
    /// The flicks: their speed, where they landed, and their direction.
    Flicks,
    /// The micros and what follows them: their count, their size, misses and slips off the target.
    Micros,
}

/// One line: the extra kills over the run, the extra score (null when the run gives no score per kill), and what was
/// assumed.
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ClickWhatIf {
    /// The part of the run the line is about.
    pub group: Group,
    /// The line's title, the change to make ("React faster").
    pub what: &'static str,
    /// The kills the change would add over the run, at the run's own pace.
    pub kills: f64,
    /// The score those kills would add at the run's points a kill; None when the run gives no score per kill.
    pub score: Option<f64>,
    /// What the line assumed, in a sentence.
    pub how: String,
}

/// One line, with the numbers its sentence gives.
#[derive(Clone, Copy, Debug)]
enum Line {
    /// Each reaction cut to `cut_s`.
    Reaction {
        /// The reaction time each reaction is cut to, in seconds.
        cut_s: f64,
    },
    /// Each confirmation before the click cut to the median, `median_s`.
    Confirmation {
        /// The run's median confirmation time, in seconds.
        median_s: f64,
    },
    /// The best 10 seconds' kills a minute, against the run's.
    BestTen {
        /// Kills a minute in the run's best 10 seconds.
        best_per_minute: f64,
        /// Kills a minute over the whole run.
        run_per_minute: f64,
    },
    /// In a hold-fire run, each time from a kill until on the next target cut to `cut_s`.
    NextTarget {
        /// The time from a kill until on the next target that each such time is cut to, in seconds.
        cut_s: f64,
    },
    /// Each flick at the speed of the fastest quarter of the flicks of its distance.
    Flick,
    /// The flicks that ended short of the target or past it.
    Land {
        /// How many flicks ended short of the target or past it.
        missed_flicks: usize,
    },
    /// The best direction's name (DIRECTIONS).
    Direction {
        /// The name of the direction the player flicks best in.
        best: &'static str,
    },
    /// The median count of micros, and the median settle time (seconds) of the kills with no more.
    FewerMicros {
        /// The median count of micro corrections a kill.
        median_micros: f64,
        /// The median settle time of the kills with no more micros than the median, in seconds.
        others_settle_s: f64,
    },
    /// The flicks that landed on the target and were still corrected.
    SmallerMicros {
        /// How many flicks landed on the target and were still corrected.
        corrected_flicks: usize,
    },
    /// The time a miss cost, and the points hitting every shot would add on its own, in a scenario whose score is
    /// scaled by accuracy.
    Miss {
        /// The time a miss cost on average, in seconds.
        per_miss_s: f64,
        /// The points hitting every shot would add on its own; None where the score is not scaled by accuracy.
        accuracy_points: Option<f64>,
    },
    /// In a hold-fire run, the time off the target after first reaching it, while firing.
    Slip,
    /// The reloads the misses forced, their time, and the points they took off, in a scenario that takes points off
    /// for a reload.
    Reload {
        /// How many reloads the misses forced.
        forced_by_misses: i64,
        /// The time those reloads took, in seconds.
        seconds: f64,
        /// The points those reloads took off; None where the scenario takes no points off for a reload.
        points_back: Option<f64>,
    },
}

/// One line's saving: seconds over the run.
struct Saving {
    /// The line, with the numbers its sentence gives.
    line: Line,
    /// The time the change would save over the run, in seconds.
    seconds: f64,
}

impl Line {
    /// The part of the run the line is about, and its title.
    fn heading(self) -> (Group, &'static str) {
        match self {
            Line::Reaction { .. } => (Group::Pace, "React faster"),
            Line::Confirmation { .. } => (Group::Pace, "Shorter confirmation"),
            Line::BestTen { .. } => (Group::Pace, "Keep up your best 10 seconds all run"),
            Line::NextTarget { .. } => (Group::Pace, "Get to the next target faster"),
            Line::Reload { .. } => (Group::Pace, "Reload less: miss less"),
            Line::Flick => (Group::Flicks, "Flick faster"),
            Line::Land { .. } => (Group::Flicks, "Fewer overflicks and underflicks"),
            Line::Direction { .. } => (Group::Flicks, "Flick every direction like your best one"),
            Line::FewerMicros { .. } => (Group::Micros, "Fewer micros"),
            Line::SmallerMicros { .. } => (Group::Micros, "Smaller micros"),
            Line::Miss { .. } => (Group::Micros, "Don't miss"),
            Line::Slip => (Group::Micros, "Don't slip off the target"),
        }
    }

    /// What the line assumed, in a sentence.
    fn how(self) -> String {
        match self {
            Line::Reaction { cut_s } => {
                format!("Each reaction cut to {}, the edge of your fastest quarter.", whole_ms(cut_s))
            }
            Line::Confirmation { median_s } => {
                format!("Each confirmation before the click cut to your median, {}.", whole_ms(median_s))
            }
            Line::BestTen { best_per_minute, run_per_minute } => format!(
                "Your best 10 seconds went at {best_per_minute:.0} kills a minute, against {run_per_minute:.0} over \
                 the run."
            ),
            Line::NextTarget { cut_s } => format!(
                "The time from each kill until on the next target cut to {}, the edge of your fastest quarter.",
                whole_ms(cut_s)
            ),
            Line::Flick => "Each flick at the speed of your fastest quarter of flicks of its distance.".into(),
            Line::Land { missed_flicks } => format!(
                "The time from the flick's end until on the target, after your {missed_flicks} overflicks and \
                 underflicks."
            ),
            Line::Direction { best } => format!(
                "Each direction's flicks at the speed of your best direction ({best}) for their distance, among \
                 directions with 5% of flicks or more."
            ),
            Line::FewerMicros { median_micros, others_settle_s } => format!(
                "Kills with more micros than your median ({median_micros}) brought to the median micro time of the \
                 others, {}.",
                whole_ms(others_settle_s)
            ),
            Line::SmallerMicros { corrected_flicks } => format!(
                "The micro time after your {corrected_flicks} flicks that landed on the target but off its center."
            ),
            Line::Miss { per_miss_s, accuracy_points } => miss_how(per_miss_s, accuracy_points),
            Line::Slip => "The time off the target after first reaching it, while firing.".into(),
            Line::Reload { forced_by_misses, seconds, points_back } => {
                reload_how(forced_by_misses, seconds, points_back)
            }
        }
    }
}

/// The miss line's sentence: what a miss cost, and in a scenario whose score is scaled by accuracy, the points
/// hitting every shot would add on its own.
fn miss_how(per_miss_s: f64, accuracy_points: Option<f64>) -> String {
    match accuracy_points {
        Some(points) => format!(
            "Each kill with a miss brought to the TTK of your clean kills of its distance, about {} a miss; \
             your score is also scaled by your accuracy, so hitting every shot would add about {points:.0} points \
             on its own.",
            whole_ms(per_miss_s)
        ),
        None => format!(
            "Each kill with a miss brought to the TTK of your clean kills of its distance, about {} a miss.",
            whole_ms(per_miss_s)
        ),
    }
}

/// The reload line's sentence: the reloads the misses forced and their time (seconds), and in a scenario that takes
/// points off for a reload, the points they would give back.
fn reload_how(forced_by_misses: i64, seconds: f64, points_back: Option<f64>) -> String {
    let reloads = if forced_by_misses == 1 { "reload" } else { "reloads" };
    let given = format!(
        "Your misses forced {forced_by_misses} {reloads} of an empty magazine, {seconds:.1} s in all. Reloads you \
         chose yourself don't show in the stats and are not counted."
    );
    match points_back {
        Some(points) => {
            format!("{given} Each reload also takes points off: about {points:.0} points back on their own.")
        }
        None => given,
    }
}

/// The `fraction` quantile of the values, interpolated linearly between the sorted values (numpy's default).
fn quantile(values: &[f64], fraction: f64) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let position = fraction * (sorted.len() - 1) as f64;
    let (below, weight) = (position.floor() as usize, position.fract());
    if below + 1 < sorted.len() { sorted[below] + weight * (sorted[below + 1] - sorted[below]) } else { sorted[below] }
}

/// The time above `cap_s`, summed over the times (seconds).
fn time_above(times_s: &[f64], cap_s: f64) -> f64 {
    times_s.iter().map(|&time_s| (time_s - cap_s).max(0.0)).sum()
}

/// Each time cut to the edge of its fastest quarter (the 25th percentile), with MIN_TIMES times or more. `line` makes
/// the line from that edge.
fn cut_to_fastest_quarter(times_s: &[f64], line: impl FnOnce(f64) -> Line) -> Option<Saving> {
    (times_s.len() >= MIN_TIMES).then(|| {
        let cut_s = quantile(times_s, FASTEST_QUARTER_OF_TIMES);
        Saving { line: line(cut_s), seconds: time_above(times_s, cut_s) }
    })
}

/// Each reaction cut to the edge of the run's fastest quarter of reactions.
fn reaction(measures: &[Measure]) -> Option<Saving> {
    let reactions_s: Vec<f64> = measures.iter().filter_map(|measure| measure.react).collect();
    cut_to_fastest_quarter(&reactions_s, |cut_s| Line::Reaction { cut_s })
}

/// Each time from a kill until on the next target cut to the edge of the run's fastest quarter of them.
fn next_target(measures: &[Measure]) -> Option<Saving> {
    let arrivals_s: Vec<f64> = measures.iter().filter_map(|measure| measure.arrive).collect();
    cut_to_fastest_quarter(&arrivals_s, |cut_s| Line::NextTarget { cut_s })
}

/// Each confirmation before the click cut to the run's median, with MIN_TIMES confirmations or more.
fn confirmation(measures: &[Measure]) -> Option<Saving> {
    let confirmations_s: Vec<f64> = measures.iter().filter_map(|measure| measure.still).collect();
    (confirmations_s.len() >= MIN_TIMES).then(|| {
        let median_s = median(&confirmations_s);
        Saving { line: Line::Confirmation { median_s }, seconds: time_above(&confirmations_s, median_s) }
    })
}

/// The best 10 s window's kill rate kept all run: the measured kills would take `kills / best` instead of `total_s`.
/// The windows start at the run's first flick or at a kill and end by the last kill; the run must span 20 s.
/// `kills_per_s` is the run's own pace.
fn best_ten(measures: &[Measure], fps: f64, kills_per_s: f64, total_s: f64) -> Option<Saving> {
    let mut kill_times_s: Vec<f64> = measures.iter().map(|measure| measure.kill_frame as f64 / fps).collect();
    kill_times_s.sort_by(f64::total_cmp);
    let first_s = measures.iter().map(|measure| measure.start_frame).min()? as f64 / fps;
    let last_s = *kill_times_s.last()?;
    if last_s - first_s < MIN_RUN_FOR_BEST_S {
        return None;
    }
    let kills_in_window = |start_s: f64| {
        kill_times_s.iter().filter(|&&kill_s| start_s < kill_s && kill_s <= start_s + BEST_WINDOW_S).count()
    };
    let best_kills = std::iter::once(first_s)
        .chain(kill_times_s.iter().copied())
        .filter(|&start_s| start_s + BEST_WINDOW_S <= last_s)
        .map(kills_in_window)
        .max()?;
    let best_per_s = best_kills as f64 / BEST_WINDOW_S;
    (best_per_s > 0.0).then(|| Saving {
        line: Line::BestTen {
            best_per_minute: SECONDS_PER_MINUTE * best_per_s,
            run_per_minute: SECONDS_PER_MINUTE * kills_per_s,
        },
        seconds: (total_s - measures.len() as f64 / best_per_s).max(0.0),
    })
}

/// A flick's way covered, from its start to where it ended (degrees), and its time (seconds).
#[derive(Clone, Copy)]
struct FlickTravel {
    /// The way the flick covered toward the target, in degrees.
    way_deg: f64,
    /// The flick's time, in seconds.
    time_s: f64,
}

impl FlickTravel {
    /// The flick's mean speed, in degrees a second.
    fn speed_deg_s(self) -> f64 {
        self.way_deg / self.time_s
    }
}

/// A kill's flick travel, when it had a main flick that covered some way.
fn flick_travel(measure: &Measure) -> Option<FlickTravel> {
    let way_deg = measure.start_distance_deg - measure.end_left;
    measure.flick.filter(|&time_s| time_s > 0.0 && way_deg > 0.0).map(|time_s| FlickTravel { way_deg, time_s })
}

/// The measures whose flick started in the distance group `group` (summary.rs: `DISTANCES`).
fn in_group(measures: &[Measure], group: (u32, u32)) -> impl Iterator<Item = &Measure> {
    measures.iter().filter(move |measure| in_distance_group(measure.start_distance_deg, group))
}

/// The flick travels in each distance group.
fn travels_by_distance(measures: &[Measure]) -> [Vec<FlickTravel>; DISTANCES.len()] {
    DISTANCES.map(|group| in_group(measures, group).filter_map(flick_travel).collect())
}

/// Each flick at the speed of the fastest quarter (the 75th percentile) of its distance group's flicks.
fn flick_faster(measures: &[Measure]) -> Option<Saving> {
    let saved_s: Vec<f64> = travels_by_distance(measures)
        .into_iter()
        .filter(|group| group.len() >= MIN_TIMES)
        .map(|group| {
            let speeds_deg_s: Vec<f64> = group.iter().map(|travel| travel.speed_deg_s()).collect();
            let fast_deg_s = quantile(&speeds_deg_s, FASTEST_QUARTER_OF_SPEEDS);
            group.iter().map(|travel| (travel.time_s - travel.way_deg / fast_deg_s).max(0.0)).sum()
        })
        .collect();
    (!saved_s.is_empty()).then(|| Saving { line: Line::Flick, seconds: saved_s.iter().sum() })
}

/// After a flick that ended short of or past the target (more than its radius from its center), the time from its end
/// until on the target.
fn land(measures: &[Measure], radius_deg: f64) -> Option<Saving> {
    let onto_target_s: Vec<f64> = measures
        .iter()
        .filter(|measure| measure.end_left.abs() > radius_deg)
        .filter_map(|measure| measure.parts.map(|parts| parts[ONTO_TARGET_PART]))
        .collect();
    (!onto_target_s.is_empty()).then(|| Saving {
        line: Line::Land { missed_flicks: onto_target_s.len() },
        seconds: onto_target_s.iter().sum(),
    })
}

/// A flick's speed against the median of its distance group's, and its time (seconds).
struct RelativeFlick {
    /// The flick's speed over the median speed of its distance group's flicks.
    relative_speed: f64,
    /// The flick's time, in seconds.
    time_s: f64,
}

/// Each direction sector's flicks (DIRECTIONS), each one's speed taken against the median of its distance group's
/// (groups of MIN_IN_GROUP or more), so near and far directions compare fairly.
fn flicks_by_direction(measures: &[Measure]) -> [Vec<RelativeFlick>; DIRECTIONS.len()] {
    let mut sectors: [Vec<RelativeFlick>; DIRECTIONS.len()] = Default::default();
    for &group in &DISTANCES {
        let flicks: Vec<(&Measure, FlickTravel)> = in_group(measures, group)
            .filter_map(|measure| flick_travel(measure).map(|travel| (measure, travel)))
            .collect();
        if flicks.len() < MIN_IN_GROUP {
            continue;
        }
        let median_deg_s = median(&flicks.iter().map(|(_, travel)| travel.speed_deg_s()).collect::<Vec<_>>());
        for (measure, travel) in flicks {
            let flick = RelativeFlick { relative_speed: travel.speed_deg_s() / median_deg_s, time_s: travel.time_s };
            sectors[direction_sector(measure.direction_deg)].push(flick);
        }
    }
    sectors
}

/// Each direction's flicks at its best direction's speed. A flick's speed is taken against its distance group's median
/// (groups of 3 or more), so near and far directions compare fairly; directions need 5% of the flicks and 3 or more.
fn direction(measures: &[Measure]) -> Option<Saving> {
    let sectors = flicks_by_direction(measures);
    let flicks: usize = sectors.iter().map(Vec::len).sum();
    let needed = (MIN_DIRECTION_SHARE * flicks as f64).ceil().max(MIN_IN_GROUP as f64) as usize;
    let speeds: Capped<(usize, f64), { DIRECTIONS.len() }> = (0..sectors.len())
        .filter(|&sector| sectors[sector].len() >= needed)
        .map(|sector| {
            let relative_speeds: Vec<f64> = sectors[sector].iter().map(|flick| flick.relative_speed).collect();
            (sector, median(&relative_speeds))
        })
        .collect();
    if speeds.len() < MIN_DIRECTIONS {
        return None;
    }
    let &(best_sector, best_speed) = speeds.iter().max_by(|a, b| a.1.total_cmp(&b.1))?;
    let saved_in_sector = |sector: usize, speed: f64| {
        sectors[sector].iter().map(|flick| flick.time_s * (1.0 - speed / best_speed)).sum::<f64>()
    };
    let seconds = speeds.iter().map(|&(sector, speed)| saved_in_sector(sector, speed)).sum();
    Some(Saving { line: Line::Direction { best: DIRECTIONS[best_sector] }, seconds })
}

/// A kill's count of micros and its settle time (seconds).
struct MicroKill {
    /// The micros (corrections) the kill took, as a float for the median.
    micros: f64,
    /// The time the micros took, in seconds.
    settle_s: f64,
}

/// The kills with more than the median count of micros, their settle time cut to the median settle time of the
/// others.
fn fewer_micros(measures: &[Measure]) -> Option<Saving> {
    let kills: Vec<MicroKill> = measures
        .iter()
        .filter_map(|measure| measure.settle.map(|settle_s| MicroKill { micros: measure.corrections as f64, settle_s }))
        .collect();
    if kills.len() < MIN_TIMES {
        return None;
    }
    let median_micros = median(&kills.iter().map(|kill| kill.micros).collect::<Vec<_>>());
    let others_settles_s: Vec<f64> =
        kills.iter().filter(|kill| kill.micros <= median_micros).map(|kill| kill.settle_s).collect();
    let others_settle_s = median(&others_settles_s);
    let seconds = kills
        .iter()
        .filter(|kill| kill.micros > median_micros)
        .map(|kill| (kill.settle_s - others_settle_s).max(0.0))
        .sum();
    Some(Saving { line: Line::FewerMicros { median_micros, others_settle_s }, seconds })
}

/// The settle time after flicks that ended on the target (neither short nor past) and were still corrected.
fn smaller_micros(measures: &[Measure], radius_deg: f64) -> Option<Saving> {
    let settles_s: Vec<f64> = measures
        .iter()
        .filter(|measure| measure.end_left.abs() <= radius_deg && measure.corrections > 0)
        .filter_map(|measure| measure.settle)
        .collect();
    (!settles_s.is_empty()).then(|| Saving {
        line: Line::SmallerMicros { corrected_flicks: settles_s.len() },
        seconds: settles_s.iter().sum(),
    })
}

/// Each kill that took more shots than a kill needs (the stats file's hits per kill, else the fewest shots of a kill),
/// brought to the median TTK of the clean kills of its distance group (3 or more of them).
fn miss(measures: &[Measure], hits_per_kill: Option<f64>, accuracy_points: Option<f64>) -> Option<Saving> {
    let needed_shots = match hits_per_kill {
        Some(hits) => (hits.round() as i64).max(1),
        None => measures.iter().filter_map(|measure| measure.shots).filter(|&shots| shots >= 1).min()?,
    };
    let (mut seconds, mut misses) = (0.0, 0);
    for &group in &DISTANCES {
        let kills: Vec<&Measure> = in_group(measures, group).collect();
        let clean_ttks_s: Vec<f64> =
            kills.iter().filter(|kill| kill.shots == Some(needed_shots)).map(|kill| kill.total).collect();
        if clean_ttks_s.len() < MIN_IN_GROUP {
            continue;
        }
        let clean_ttk_s = median(&clean_ttks_s);
        for kill in &kills {
            if let Some(shots) = kill.shots.filter(|&shots| shots > needed_shots) {
                seconds += (kill.total - clean_ttk_s).max(0.0);
                misses += shots - needed_shots;
            }
        }
    }
    (misses > 0).then(|| Saving { line: Line::Miss { per_miss_s: seconds / misses as f64, accuracy_points }, seconds })
}

/// In a hold-fire run, the time off the target after first reaching it.
fn slip(measures: &[Measure]) -> Option<Saving> {
    let off_target_s: Vec<f64> =
        measures.iter().filter(|measure| measure.hold.is_some()).map(|measure| measure.off).collect();
    (!off_target_s.is_empty()).then(|| Saving { line: Line::Slip, seconds: off_target_s.iter().sum() })
}

/// The reload time the misses forced: the run's forced reloads against the same run with each kill's shots cut to its
/// hits (src/reload.rs).
fn fewer_reloads(cost: &ReloadCost) -> Option<Saving> {
    let clean = cost.clean.as_ref()?;
    let forced_by_misses = cost.run.count - clean.count;
    let seconds = cost.run.seconds - clean.seconds;
    let points_back = cost.run.score_lost.map(|lost| lost - clean.score_lost.unwrap_or(0.0));
    (forced_by_misses > 0).then_some(Saving { line: Line::Reload { forced_by_misses, seconds, points_back }, seconds })
}

/// The points hitting every shot would add, when the stats file shows the score is the kills times whole points
/// times the accuracy (and not whole points a kill).
fn accuracy_points(summary: &Summary) -> Option<f64> {
    let (score, accuracy, kills) = (summary.score?, summary.accuracy?, summary.kills as f64);
    if kills <= 0.0 || accuracy <= 0.0 || accuracy >= 1.0 {
        return None;
    }
    let whole = |value: f64| (value - value.round()).abs() < WHOLE_TOLERANCE * value.abs().max(1.0);
    (whole(score / (kills * accuracy)) && !whole(score / kills)).then(|| score / accuracy - score)
}

/// The run's what-if lines, biggest first, without those under half a kill. A hold-fire run has no click to wait for
/// or correct before: it gets the time to the next target and the time off it while firing instead of the
/// confirmation, the micros and the misses. `reload`: what reloading cost the run, when its weapon's magazine runs out.
pub fn click_what_if(
    measures: &[Measure],
    summary: &Summary,
    radius_deg: f64,
    fps: f64,
    hits_per_kill: Option<f64>,
    reload: Option<&ReloadCost>,
) -> Vec<ClickWhatIf> {
    let total_s: f64 = measures.iter().map(|measure| measure.total).sum();
    if measures.is_empty() || total_s <= 0.0 {
        return Vec::new();
    }
    let kills_per_s = measures.len() as f64 / total_s;
    let score_per_kill = summary.score.filter(|_| summary.kills > 0).map(|score| score / summary.kills as f64);
    let (hold, click) = (summary.mode == Mode::Hold, summary.mode != Mode::Hold);
    let savings = [
        reaction(measures),
        click.then(|| confirmation(measures)).flatten(),
        best_ten(measures, fps, kills_per_s, total_s),
        hold.then(|| next_target(measures)).flatten(),
        flick_faster(measures),
        land(measures, radius_deg),
        direction(measures),
        click.then(|| fewer_micros(measures)).flatten(),
        click.then(|| smaller_micros(measures, radius_deg)).flatten(),
        click.then(|| miss(measures, hits_per_kill, accuracy_points(summary))).flatten(),
        hold.then(|| slip(measures)).flatten(),
        reload.and_then(fewer_reloads),
    ];
    let mut lines: Vec<ClickWhatIf> = savings
        .into_iter()
        .flatten()
        .filter_map(|saving| {
            let kills = saving.seconds.min(total_s) * kills_per_s;
            (kills >= MIN_KILLS_SHOWN).then(|| {
                let (group, what) = saving.line.heading();
                let score = score_per_kill.map(|per_kill| kills * per_kill);
                ClickWhatIf { group, what, kills, score, how: saving.line.how() }
            })
        })
        .collect();
    lines.sort_by(|a, b| b.kills.total_cmp(&a.kills));
    lines
}

/// Checks each line on a made-up run worked out by hand.
#[cfg(test)]
mod tests {
    use super::*;

    /// A made-up flick: distance, direction, kill time, reaction, flick time, where the flick ended, micros, settle,
    /// confirmation, shots.
    #[allow(clippy::too_many_arguments)]
    fn flick(
        start_distance_deg: f64,
        direction_deg: f64,
        total_s: f64,
        react_s: f64,
        flick_s: f64,
        end_left: f64,
        corrections: usize,
        settle_s: f64,
        still_s: f64,
        shots: i64,
    ) -> Measure {
        Measure {
            kill_number: 0,
            shots: Some(shots),
            start_distance_deg,
            direction_deg,
            total: total_s,
            react: Some(react_s),
            flick: Some(flick_s),
            peak: 0.0,
            end_left,
            end_off: end_left.abs(),
            arrive: Some(react_s + flick_s + 0.05),
            dwell: None,
            past: 0.0,
            corrections,
            click_speed: 0.0,
            click_off: 0.0,
            click_off_xy: (0.0, 0.0),
            settle: Some(settle_s),
            still: Some(still_s),
            start_frame: 0,
            kill_frame: 0,
            spawned: false,
            hold: Some(settle_s + still_s),
            breaks: 0,
            off: 0.02,
            parts: Some([react_s, flick_s, 0.05, settle_s, still_s]),
            speed: None,
            reloads: None,
            reload_time: None,
        }
    }

    /// Four 6-degree flicks right, four left (0 = right, 180 = left): the radius is 0.5.
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

    /// Asserts the two values are within 1e-9.
    fn close(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "{a} against {b}");
    }

    /// `quantile` interpolates between the sorted values as NumPy's default does.
    #[test]
    fn quantiles_as_numpy_gives_them() {
        close(quantile(&[4.0, 1.0, 3.0, 2.0], 0.25), 1.75);
        close(quantile(&[4.0, 1.0, 3.0, 2.0], 0.75), 3.25);
        close(quantile(&[5.0], 0.25), 5.0);
    }

    /// The reaction, confirmation and next-target lines save the time above their cut.
    #[test]
    fn pace_lines() {
        let measures = run();
        // reactions 0.10 0.10 0.12 0.12 0.14 0.14 0.20 0.30: the 25th percentile is 0.115
        let saving = reaction(&measures).unwrap();
        let Line::Reaction { cut_s } = saving.line else { panic!("{:?}", saving.line) };
        close(cut_s, 0.115);
        close(saving.seconds, 0.005 + 0.005 + 0.025 + 0.025 + 0.085 + 0.185);
        // confirmations 0.05 0.05 0.06 0.06 0.07 0.08 0.10 0.12: the median is 0.065
        let saving = confirmation(&measures).unwrap();
        close(saving.seconds, 0.005 + 0.015 + 0.035 + 0.055);
        // onto the target: reaction + flick + 0.05
        let saving = next_target(&measures).unwrap();
        let arrivals_s: Vec<f64> = measures.iter().map(|measure| measure.arrive.unwrap()).collect();
        close(saving.seconds, time_above(&arrivals_s, quantile(&arrivals_s, 0.25)));
    }

    /// The best 10 seconds' rate kept all run saves the time the run spent under it.
    #[test]
    fn the_best_ten_seconds() {
        // 30 kills a second apart, then 10 kills 3 s apart: 10 kills in the best 10 s
        let mut measures = Vec::new();
        let mut time_s = 0.0;
        for i in 0..40 {
            let ttk_s = if i < 30 { 1.0 } else { 3.0 };
            time_s += ttk_s;
            let mut measure = flick(6.0, 0.0, ttk_s, 0.1, 0.1, 0.0, 0, 0.0, 0.0, 1);
            measure.start_frame = ((time_s - measure.total) * 60.0) as i64;
            measure.kill_frame = (time_s * 60.0) as i64;
            measures.push(measure);
        }
        let total_s = 60.0;
        let saving = best_ten(&measures, 60.0, 40.0 / total_s, total_s).unwrap();
        // at 1 kill a second the 40 kills take 40 s: 20 s saved
        let Line::BestTen { best_per_minute, .. } = saving.line else { panic!("{:?}", saving.line) };
        close(best_per_minute, 60.0);
        close(saving.seconds, 20.0);
    }

    /// The flick speed, landing and direction lines on the made-up run.
    #[test]
    fn flick_lines() {
        let measures = run();
        // speeds (way / time), right: 60, 41.67, 46.67, 29; left: 50, 40, 30, 25. All in the 5-10 degree group.
        let speeds = [60.0, 5.0 / 0.12, 7.0 / 0.15, 5.8 / 0.2, 6.0 / 0.12, 6.0 / 0.15, 6.0 / 0.2, 6.0 / 0.24];
        let fast_deg_s = quantile(&speeds, 0.75);
        let want: f64 = measures
            .iter()
            .map(|measure| {
                flick_travel(measure).map_or(0.0, |travel| (travel.time_s - travel.way_deg / fast_deg_s).max(0.0))
            })
            .sum();
        close(flick_faster(&measures).unwrap().seconds, want);
        // two flicks ended past the radius 0.5 (one short, one past): their 0.05 s onto the target
        let saving = land(&measures, 0.5).unwrap();
        close(saving.seconds, 0.10);
        let Line::Land { missed_flicks } = saving.line else { panic!("{:?}", saving.line) };
        assert_eq!(missed_flicks, 2);
        // right is the faster direction: left's flicks take its median speed ratio
        let mid = median(&speeds);
        let right = median(&speeds[..4].iter().map(|speed| speed / mid).collect::<Vec<_>>());
        let left = median(&speeds[4..].iter().map(|speed| speed / mid).collect::<Vec<_>>());
        let saving = direction(&measures).unwrap();
        let Line::Direction { best } = saving.line else { panic!("{:?}", saving.line) };
        assert_eq!(best, "right");
        close(saving.seconds, (0.12 + 0.15 + 0.20 + 0.24) * (1.0 - left / right));
    }

    /// The fewer-micros, smaller-micros, miss and slip lines on the made-up run.
    #[test]
    fn micro_lines() {
        let measures = run();
        // micros 0 1 2 3 0 1 1 2: the median is 1; settle of those with 1 or fewer: 0, 0.04, 0.02, 0.04, 0.06
        let saving = fewer_micros(&measures).unwrap();
        let Line::FewerMicros { others_settle_s, .. } = saving.line else { panic!("{:?}", saving.line) };
        close(others_settle_s, 0.04);
        close(saving.seconds, (0.08 - 0.04) + (0.12 - 0.04) + (0.10 - 0.04));
        // landed (|end_left| <= 0.5) with micros: 0.12 + 0.04 + 0.06 + 0.10
        close(smaller_micros(&measures, 0.5).unwrap().seconds, 0.32);
        // one hit a kill; clean kills' median TTK 0.5; the 2-shot kill (0.6) and the 3-shot kill (0.6): 3 misses
        let saving = miss(&measures, Some(1.0), None).unwrap();
        close(saving.seconds, 0.2);
        let Line::Miss { per_miss_s, .. } = saving.line else { panic!("{:?}", saving.line) };
        close(per_miss_s, 0.2 / 3.0);
        close(slip(&measures).unwrap().seconds, 0.16);
    }

    /// The reload line saves the reloads the misses forced, their time and their points, and says so.
    #[test]
    fn reload_line() {
        use crate::reload::Reloads;
        // 3 forced reloads, 1 without the misses: 2 saved, a second, and 50 of the 75 points they took off
        let run = Reloads { count: 3, seconds: 1.5, score_lost: Some(75.0) };
        let clean = Some(Reloads { count: 1, seconds: 0.5, score_lost: Some(25.0) });
        let saving = fewer_reloads(&ReloadCost { per_kill: Vec::new(), run, clean }).unwrap();
        close(saving.seconds, 1.0);
        let Line::Reload { forced_by_misses, points_back, .. } = saving.line else { panic!("{:?}", saving.line) };
        assert_eq!(forced_by_misses, 2);
        assert_eq!(points_back, Some(50.0));
        assert!(saving.line.how().contains("2 reloads of an empty magazine, 1.0 s"));
    }
}
