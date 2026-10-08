//! A tracking run's checks: each a Work on / Fine verdict, like a clicking run's (src/summary.rs `judge`).
//!
//! In: the tracking run's summary (src/tracking.rs): its what-if estimates and its time on target second by second.
//! Out: the report's checks (`Issue`), which the run page shows beside the video. Each what-if check flags a habit whose
//! fix would add `GAIN_FLAGGED` or more of the run's time on target: one line for every scenario, since the gain is a
//! share of the run whatever its bots.

use crate::summary::{Issue, check, percent_number};
use crate::tracking::{
    BEST_TEN_SECONDS, EVERY_DIRECTION, FASTER_SWITCH, GET_BACK, NO_LEADING, NO_SLIPS, NO_TRAILING, NOT_THROWN,
    TrackSummary,
};

/// A what-if gain at or past this share of the run is flagged: 5 points of time on target.
const GAIN_FLAGGED: f64 = 0.05;
/// A last third this much less on target than the first (a share of the time) is flagged: 5 points.
const PACE_DROP_FLAGGED: f64 = 0.05;
/// The fewest seconds the pacing check needs, so each third has at least three.
const MIN_PACE_SECONDS: usize = 9;
/// The parts of the run the pacing check compares: its first and its last third.
const THIRDS: usize = 3;
/// What the what-if checks say of their line, after why they matter.
const FLAGGED_AT: &str = "Flagged when fixing it would add 5% or more of the run on target.";

/// When a what-if check applies to a run.
#[derive(Clone, Copy)]
enum Applies {
    /// Every run: a missing what-if gains nothing.
    Always,
    /// Runs whose bot's motion was measured (the time off it ahead, behind, after turns, by direction).
    Motion,
    /// Runs that have the what-if (a best 10 seconds needs 10 seconds; a switch needs bots that die).
    WhenThere,
}

/// A what-if check: its issue number, the what-if it reads, its heading, the sentence its value goes in, why it
/// matters, and when it applies.
struct GainCheck {
    /// The check's number, past the clicking checks' (src/summary.rs).
    issue: u32,
    /// The what-if it reads (src/tracking.rs).
    what: &'static str,
    /// Its heading on the run page.
    title: &'static str,
    /// The value's sentence, with the gain as a whole percentage in place of `{}`.
    value: &'static str,
    /// Why it matters.
    why: &'static str,
    /// When it applies.
    applies: Applies,
}

/// Every what-if check.
const GAIN_CHECKS: [GainCheck; 8] = [
    GainCheck {
        issue: 101,
        what: NO_LEADING,
        title: "Leading",
        value: "{}% of the run off the bot ahead of it",
        why: "Your crosshair ran past the bot's leading edge.",
        applies: Applies::Motion,
    },
    GainCheck {
        issue: 102,
        what: NO_TRAILING,
        title: "Trailing",
        value: "{}% of the run off the bot behind it",
        why: "Your crosshair fell behind the bot.",
        applies: Applies::Motion,
    },
    GainCheck {
        issue: 103,
        what: NOT_THROWN,
        title: "Thrown by turns",
        value: "{}% of the run off the bot just after it turned",
        why: "The time off the bot in the 0.4 s after each of its direction changes.",
        applies: Applies::Motion,
    },
    GainCheck {
        issue: 104,
        what: GET_BACK,
        title: "Slow to get back",
        value: "getting back on twice as fast would add {}% on target",
        why: "Half the time off the bot in drops longer than 0.1 s.",
        applies: Applies::Always,
    },
    GainCheck {
        issue: 105,
        what: NO_SLIPS,
        title: "Slips",
        value: "{}% of the run off the bot in slips shorter than 0.1 s",
        why: "Brief slips off the bot, too short to count as losing it.",
        applies: Applies::Always,
    },
    GainCheck {
        issue: 106,
        what: EVERY_DIRECTION,
        title: "Direction bias",
        value: "tracking every direction like your best one would add {}% on target",
        why: "Some directions of the bot's motion kept you on it less than your best one (among those it moved in 5% \
              of the time or more).",
        applies: Applies::Motion,
    },
    GainCheck {
        issue: 107,
        what: BEST_TEN_SECONDS,
        title: "Consistency",
        value: "keeping up your best 10 seconds all run would add {}% on target",
        why: "Your best 10 seconds against the whole run.",
        applies: Applies::WhenThere,
    },
    GainCheck {
        issue: 108,
        what: FASTER_SWITCH,
        title: "Switching",
        value: "getting onto the next bot 100 ms faster would add {}% on target",
        why: "100 ms less per switch between bots that die.",
        applies: Applies::WhenThere,
    },
];

/// The tracking run's checks: one per what-if check that applies, then its pacing; none without the what-ifs (no
/// run).
pub fn judge(summary: &TrackSummary) -> Vec<Issue> {
    let Some(what_ifs) = summary.what_if.as_ref() else { return Vec::new() };
    let motion = summary.motion.as_ref().is_some_and(|motion| motion.counts.is_some());
    let gain_of = |what: &str| what_ifs.iter().find(|what_if| what_if.what == what).map(|what_if| what_if.gain);
    let mut checks: Vec<Issue> = GAIN_CHECKS
        .iter()
        .filter_map(|gain_check| {
            let gain = match gain_check.applies {
                Applies::Always => gain_of(gain_check.what).unwrap_or(0.0),
                Applies::Motion if motion => gain_of(gain_check.what).unwrap_or(0.0),
                Applies::Motion => return None,
                Applies::WhenThere => gain_of(gain_check.what)?,
            };
            let value = gain_check.value.replace("{}", &percent_number(gain));
            let why = format!("{} {FLAGGED_AT}", gain_check.why);
            Some(check(gain_check.issue, gain_check.title, value, gain >= GAIN_FLAGGED, why))
        })
        .collect();
    checks.extend(pacing_drop(&summary.per_second));
    checks
}

/// The time on target in the run's first third against its last (issue 109), from each second's [share on a target,
/// share switching]: each third's time on the bot over its time tracking, switching left out. None for a run shorter
/// than `MIN_PACE_SECONDS` or a third with no tracking.
fn pacing_drop(per_second: &[[f64; 2]]) -> Option<Issue> {
    if per_second.len() < MIN_PACE_SECONDS {
        return None;
    }
    let third = per_second.len() / THIRDS;
    let on_share = |seconds: &[[f64; 2]]| {
        let tracking: f64 = seconds.iter().map(|second| 1.0 - second[1]).sum();
        (tracking > 0.0).then(|| seconds.iter().map(|second| second[0]).sum::<f64>() / tracking)
    };
    let first = on_share(&per_second[..third])?;
    let last = on_share(&per_second[per_second.len() - third..])?;
    Some(check(
        109,
        "Pacing drop",
        format!("{}% on target in the first third, {}% in the last", percent_number(first), percent_number(last)),
        first - last >= PACE_DROP_FLAGGED,
        "Flagged when the last third is 5 points or more less on target than the first.",
    ))
}

/// Tests of the tracking checks.
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use crate::matching::KillSource;
    use crate::summary::Flag;
    use crate::tracking::{WhatIf, unmeasured_summary};

    /// A summary of a run with nothing measured.
    fn unmeasured() -> TrackSummary {
        unmeasured_summary(&HashMap::new(), KillSource::Video)
    }

    /// A summary with these what-ifs (gains as shares of the run) and seconds, its motion not measured.
    fn summary(what_ifs: &[(&'static str, f64)], per_second: Vec<[f64; 2]>) -> TrackSummary {
        let mut summary = unmeasured();
        summary.what_if =
            Some(what_ifs.iter().map(|&(what, gain)| WhatIf { what, gain, how: String::new() }).collect());
        summary.per_second = per_second;
        summary
    }

    /// The flag and value of the check titled `title`, if the run has it.
    fn verdict(checks: &[Issue], title: &str) -> Option<(Flag, String)> {
        checks.iter().find(|check| check.title == title).map(|check| (check.flag, check.value.clone()))
    }

    /// A gain of 5 points or more is flagged, less is fine; the checks that need the motion are left out without it,
    /// a best 10 seconds and a switch only when their what-if is there, and getting back reads 0 when it is missing.
    #[test]
    fn gains_of_five_points_are_flagged() {
        let checks = judge(&summary(&[(NO_SLIPS, 0.05), (BEST_TEN_SECONDS, 0.049)], Vec::new()));
        assert_eq!(verdict(&checks, "Slips").map(|(flag, _)| flag), Some(Flag::Attention));
        assert_eq!(verdict(&checks, "Consistency").map(|(flag, _)| flag), Some(Flag::Fine));
        assert_eq!(
            verdict(&checks, "Slow to get back"),
            Some((Flag::Fine, "getting back on twice as fast would add 0% on target".into()))
        );
        for left_out in ["Leading", "Trailing", "Thrown by turns", "Direction bias", "Switching", "Pacing drop"] {
            assert!(verdict(&checks, left_out).is_none(), "{left_out}");
        }
        assert!(judge(&unmeasured()).is_empty(), "no run, no checks");
    }

    /// The last third 5 points or more less on target than the first is flagged; the time switching between bots is
    /// left out of each third's share.
    #[test]
    fn a_drop_of_five_points_by_the_last_third_is_flagged() {
        let seconds = |first: [f64; 2], last: [f64; 2]| [vec![first; 3], vec![[0.5, 0.0]; 3], vec![last; 3]].concat();
        let dropped = judge(&summary(&[], seconds([0.9, 0.0], [0.85, 0.0])));
        assert_eq!(
            verdict(&dropped, "Pacing drop"),
            Some((Flag::Attention, "90% on target in the first third, 85% in the last".into()))
        );
        // half of each last second switching: 0.45 on target of the 0.5 tracking is 90%
        let held = judge(&summary(&[], seconds([0.9, 0.0], [0.45, 0.5])));
        assert_eq!(verdict(&held, "Pacing drop").map(|(flag, _)| flag), Some(Flag::Fine));
        assert!(verdict(&judge(&summary(&[], vec![[0.9, 0.0]; 8])), "Pacing drop").is_none(), "too short");
    }
}
