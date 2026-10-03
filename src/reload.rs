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

use serde::Serialize;

use crate::scenario::AmmoRules;

/// One kill's forced reloads: how many, and their time in seconds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct KillReloads {
    pub reloads: i64,
    pub seconds: f64,
}

/// Forced reloads over a run: how many, their time in seconds, and the points they took off (None when the scenario
/// takes none for a reload).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Reloads {
    pub count: i64,
    pub seconds: f64,
    pub score_lost: Option<f64>,
}

/// What reloading cost a run: each kill's forced reloads, the run's, and the run's with every miss taken out (each
/// kill's shots cut to its hits; None when the hits are not known).
#[derive(Clone, Debug)]
pub struct ReloadCost {
    pub per_kill: Vec<KillReloads>,
    pub run: Reloads,
    pub clean: Option<Reloads>,
}

/// Each kill's forced reloads, the kills' shots taken in the run's order.
pub fn forced_reloads(rules: &AmmoRules, shots: &[i64]) -> Vec<KillReloads> {
    let mut ammo = rules.magazine;
    shots
        .iter()
        .map(|&n| {
            let mut kill = KillReloads::default();
            for _ in 0..n {
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
    let count = kills.iter().map(|k| k.reloads).sum();
    Reloads {
        count,
        seconds: kills.iter().map(|k| k.seconds).sum(),
        score_lost: (rules.score_loss != 0.0).then_some(count as f64 * rules.score_loss),
    }
}

/// What reloading cost the run: from each kill's shots, and its hits (one value a kill) when they are known.
pub fn reload_cost(rules: &AmmoRules, shots: &[i64], hits: Option<&[i64]>) -> ReloadCost {
    let per_kill = forced_reloads(rules, shots);
    let clean = hits.map(|h| {
        let cut: Vec<i64> = shots.iter().zip(h).map(|(&s, &h)| s.min(h)).collect();
        totals(rules, &forced_reloads(rules, &cut))
    });
    ReloadCost { run: totals(rules, &per_kill), per_kill, clean }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 1w2ts reload's weapon: 3 rounds, a kill fills it again, half a second to reload.
    fn rules(on_kill: i64) -> AmmoRules {
        AmmoRules { magazine: 3, per_shot: 1, on_kill, from_empty: 0.5, from_partial: 0.4, score_loss: 0.0 }
    }

    fn counts(k: &[KillReloads]) -> Vec<i64> {
        k.iter().map(|k| k.reloads).collect()
    }

    #[test]
    fn a_kill_fills_the_magazine() {
        // 3 shots empty it, but the kill on the last one fills it: no reload, ever
        assert_eq!(counts(&forced_reloads(&rules(4), &[3, 3, 1, 2, 3])), vec![0; 5]);
    }

    #[test]
    fn an_empty_magazine_forces_a_reload() {
        // 4 shots: the 4th waits for a reload; 7 shots: the 4th and the 7th
        let k = forced_reloads(&rules(4), &[4, 1, 7]);
        assert_eq!(counts(&k), vec![1, 0, 2]);
        assert_eq!(k[2].seconds, 1.0);
        // a kill that puts back 1. 4 shots: the 4th waits, 2 left, +1 = 3; 2 shots: 1, +1 = 2; 3 shots: the 3rd waits,
        // 2 left, +1 = 3; 2 shots: 1, +1 = 2
        let k = forced_reloads(&rules(1), &[4, 2, 3, 2]);
        assert_eq!(counts(&k), vec![1, 0, 1, 0]);
    }

    #[test]
    fn a_kill_on_the_last_round_ends_the_reload() {
        // no ammo back on a kill: the magazine empties on the 3rd kill's shot, and the 4th kill's shot waits for the
        // reload; with a kill that puts back 1 (the Flow Fix 2 scenarios' one-round magazine) it never waits
        assert_eq!(counts(&forced_reloads(&rules(0), &[1, 1, 1, 1])), vec![0, 0, 0, 1]);
        let one = AmmoRules { magazine: 1, on_kill: 1, ..rules(1) };
        assert_eq!(counts(&forced_reloads(&one, &[1, 1, 1, 2])), vec![0, 0, 0, 1]);
    }

    #[test]
    fn part_used_and_points() {
        // 2 ammo a shot from 3: one shot leaves 1, too few for the next: a reload from part-used
        let r = AmmoRules { per_shot: 2, score_loss: 25.0, ..rules(0) };
        let c = reload_cost(&r, &[2, 1], Some(&[1, 1]));
        assert_eq!(counts(&c.per_kill), vec![1, 1]);
        assert_eq!(c.run, Reloads { count: 2, seconds: 0.8, score_lost: Some(50.0) });
        // without the miss: 1 shot (1 left), then 1 shot waits
        assert_eq!(c.clean, Some(Reloads { count: 1, seconds: 0.4, score_lost: Some(25.0) }));
    }
}
