//! The reloads a clicking run's magazine forced (src/scenario.rs: `AmmoRules`), worked out from each kill's shots.
//!
//! The magazine starts full; each shot uses the weapon's ammo a shot, and a kill puts its ammo back (up to a full
//! magazine). When the magazine has too little left for a shot, the weapon reloads (the time from empty, or from
//! part-used when some ammo is left) and the magazine is full again. A kill's ammo comes before that check: a kill on
//! the magazine's last shot that fills it again forces no reload, whatever CancelReloadOnKill says (KovaaK's own stats:
//! in 164 runs of 1w4ts reload, where a kill fills the magazine of 3, the kill after a 3-shot kill came no later than
//! after a 1-shot kill). A reload is counted in the kill whose shot waited for it.
//!
//! What can't be seen: reloads the player chose (they don't show in the stats), and shots after the last kill. So the
//! counts are the forced reloads only. Each one counts in full, though the player can aim while it runs: a ceiling.
//!
//! In: the weapon's ammo rules (scenario.rs) and each kill's shots and hits (stats_file.rs). Out: the reloads in the
//! clicking report (review.rs), its summary's time budget (summary.rs) and its what-if (what_if.rs).

use serde::Serialize;

use crate::scenario::AmmoRules;

/// One kill's forced reloads: how many, and their time in seconds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct KillReloads {
    /// The reloads this kill's shots waited for.
    pub reloads: i64,
    /// Their time added up, in seconds.
    pub seconds: f64,
}

/// Forced reloads over a run: how many, their time in seconds, and the points they took off (None when the scenario
/// takes none for a reload).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Reloads {
    /// The forced reloads in the run.
    pub count: i64,
    /// Their time added up, in seconds.
    pub seconds: f64,
    /// The points they took off: the count times the scenario's loss a reload; None when it takes none.
    pub score_lost: Option<f64>,
}

/// What reloading cost a run: each kill's forced reloads, the run's, and the run's with every miss taken out (each
/// kill's shots cut to its hits; None when the hits are not known).
#[derive(Clone, Debug)]
pub struct ReloadCost {
    /// Each kill's forced reloads, in the run's order.
    pub per_kill: Vec<KillReloads>,
    /// The run's forced reloads with the shots as taken.
    pub run: Reloads,
    /// The run's forced reloads had no shot missed; None when the hits are not known.
    pub clean: Option<Reloads>,
}

/// Each kill's forced reloads, from each kill's shots in the run's order (one value a kill).
pub fn forced_reloads(rules: &AmmoRules, shots: &[i64]) -> Vec<KillReloads> {
    let mut ammo = rules.magazine;
    shots
        .iter()
        .map(|&kill_shots| {
            let mut kill = KillReloads::default();
            for _ in 0..kill_shots {
                if ammo < rules.per_shot {
                    kill.reloads += 1;
                    kill.seconds += if ammo <= 0 { rules.from_empty } else { rules.from_partial };
                    ammo = rules.magazine;
                }
                ammo -= rules.per_shot;
            }
            ammo = (ammo + rules.on_kill).min(rules.magazine);
            kill
        })
        .collect()
}

/// The run's totals of the kills' forced reloads.
fn totals(rules: &AmmoRules, kills: &[KillReloads]) -> Reloads {
    let count = kills.iter().map(|kill| kill.reloads).sum();
    Reloads {
        count,
        seconds: kills.iter().map(|kill| kill.seconds).sum(),
        score_lost: (rules.score_loss != 0.0).then_some(count as f64 * rules.score_loss),
    }
}

/// What reloading cost the run: from each kill's shots, and its hits (one value a kill) when they are known.
pub fn reload_cost(rules: &AmmoRules, shots: &[i64], hits: Option<&[i64]>) -> ReloadCost {
    let per_kill = forced_reloads(rules, shots);
    let clean = hits.map(|hits| {
        let cut: Vec<i64> = shots.iter().zip(hits).map(|(&kill_shots, &kill_hits)| kill_shots.min(kill_hits)).collect();
        totals(rules, &forced_reloads(rules, &cut))
    });
    ReloadCost { run: totals(rules, &per_kill), per_kill, clean }
}

/// Checks the forced reloads on short runs worked out by hand.
#[cfg(test)]
mod tests {
    use super::*;

    /// 1w2ts reload's weapon: 3 rounds, one a shot, `on_kill` rounds back on a kill, half a second to reload from empty
    /// (0.4 s from part-used), no points lost.
    fn rules(on_kill: i64) -> AmmoRules {
        AmmoRules { magazine: 3, per_shot: 1, on_kill, from_empty: 0.5, from_partial: 0.4, score_loss: 0.0 }
    }

    /// Each kill's count of forced reloads.
    fn counts(kills: &[KillReloads]) -> Vec<i64> {
        kills.iter().map(|kill| kill.reloads).collect()
    }

    /// When each kill fills the magazine again and none takes more shots than it holds, no shot ever waits.
    #[test]
    fn a_kill_fills_the_magazine() {
        // 3 shots empty it, but the kill on the last one fills it: no reload, ever
        assert_eq!(counts(&forced_reloads(&rules(4), &[3, 3, 1, 2, 3])), vec![0; 5]);
    }

    /// A shot with the magazine empty waits for a reload, counted in that shot's kill.
    #[test]
    fn an_empty_magazine_forces_a_reload() {
        // 4 shots: the 4th waits for a reload; 7 shots: the 4th and the 7th
        let kills = forced_reloads(&rules(4), &[4, 1, 7]);
        assert_eq!(counts(&kills), vec![1, 0, 2]);
        assert_eq!(kills[2].seconds, 1.0);
        // a kill that puts back 1. 4 shots: the 4th waits, 2 left, +1 = 3; 2 shots: 1, +1 = 2; 3 shots: the 3rd waits,
        // 2 left, +1 = 3; 2 shots: 1, +1 = 2
        let kills = forced_reloads(&rules(1), &[4, 2, 3, 2]);
        assert_eq!(counts(&kills), vec![1, 0, 1, 0]);
    }

    /// A kill's ammo comes back before the next shot's check, so a kill on the last round forces no reload.
    #[test]
    fn a_kill_on_the_last_round_ends_the_reload() {
        // no ammo back on a kill: the magazine empties on the 3rd kill's shot, and the 4th kill's shot waits for the
        // reload; with a kill that puts back 1 (the Flow Fix 2 scenarios' one-round magazine) it never waits
        assert_eq!(counts(&forced_reloads(&rules(0), &[1, 1, 1, 1])), vec![0, 0, 0, 1]);
        let one = AmmoRules { magazine: 1, on_kill: 1, ..rules(1) };
        assert_eq!(counts(&forced_reloads(&one, &[1, 1, 1, 2])), vec![0, 0, 0, 1]);
    }

    /// A reload with ammo left takes the part-used time, each costs the points lost, and the clean count leaves the
    /// misses out.
    #[test]
    fn part_used_and_points() {
        // 2 ammo a shot from 3: one shot leaves 1, too few for the next: a reload from part-used
        let two_a_shot = AmmoRules { per_shot: 2, score_loss: 25.0, ..rules(0) };
        let cost = reload_cost(&two_a_shot, &[2, 1], Some(&[1, 1]));
        assert_eq!(counts(&cost.per_kill), vec![1, 1]);
        assert_eq!(cost.run, Reloads { count: 2, seconds: 0.8, score_lost: Some(50.0) });
        // without the miss: 1 shot (1 left), then 1 shot waits
        assert_eq!(cost.clean, Some(Reloads { count: 1, seconds: 0.4, score_lost: Some(25.0) }));
    }
}
