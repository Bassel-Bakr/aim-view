//! The forced reloads (src/reload.rs) on real runs of scenarios whose magazine runs out, reviewed with their stats files
//! into test_out/reload_runs/<run>/ (`aimview-tool review <video> --out <that folder> --stats-file <csv>`), with the
//! ammo rules read from the scenario's file. `--nocapture` prints each run's reloads and its what-if line.

use std::fs;
use std::path::{Path, PathBuf};

use aimview::review::{KillTimes, review_clicks};
use aimview::scenario::facts;
use aimview::track::Tracks;

const KOVAAK: &str = r"C:\Program Files (x86)\Steam\steamapps";
const RUNS: [&str; 1] = ["Pasu_Reload_Goated_-_112_-_2026.08.24-02.21.32"];

/// The scenario's file: the user's own, else the workshop's.
fn scenario_file(name: &str) -> Option<PathBuf> {
    let own = Path::new(KOVAAK).join(r"common\FPSAimTrainer\FPSAimTrainer\Saved\SaveGames\Scenarios").join(format!("{name}.sce"));
    if own.exists() {
        return Some(own);
    }
    fs::read_dir(Path::new(KOVAAK).join(r"workshop\content\824270"))
        .ok()?
        .flatten()
        .map(|item| item.path().join(format!("{name}.sce")))
        .find(|p| p.exists())
}

#[test]
fn forced_reloads_on_real_runs() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_out/reload_runs");
    for run in RUNS {
        let dir = root.join(run);
        let (Ok(tracks), Ok(report)) = (fs::read_to_string(dir.join("tracks.json")), fs::read_to_string(dir.join("report.json")))
        else {
            eprintln!("no {}", dir.display());
            continue;
        };
        let tracks: Tracks = serde_json::from_str(&tracks).unwrap();
        let report: serde_json::Value = serde_json::from_str(&report).unwrap();
        let stats = report["stats"].as_str().unwrap();
        let path = Path::new(KOVAAK).join(r"common\FPSAimTrainer\FPSAimTrainer\stats").join(stats);
        let scenario = stats.split(" - Challenge - ").next().unwrap();
        let (Ok(text), Some(sce)) = (fs::read(&path).map(|b| String::from_utf8_lossy(&b).into_owned()), scenario_file(scenario)) else {
            eprintln!("no stats file or scenario file for {run}");
            continue;
        };
        let rules = facts(&String::from_utf8_lossy(&fs::read(sce).unwrap())).reload.expect("a magazine that runs out");
        let got = review_clicks(&tracks, KillTimes::Stats { name: stats, text: &text }, report["video"].as_str().unwrap(), None, Some(&rules))
            .unwrap()
            .report;
        let s = &got.summary;
        let reloads = s.reloads.as_ref().unwrap();
        eprintln!("{scenario} ({} kills, {} measured): {rules:?}", s.kills, s.measured);
        eprintln!("  forced reloads: {reloads:?}");
        for m in got.flicks.iter().filter(|m| m.reloads.is_some_and(|n| n > 0)) {
            eprintln!("  kill {}: {:?} shots, {:?} reloads, {:?} s", m.kill_number, m.shots, m.reloads, m.reload_time);
        }
        let line = s.what_if.iter().find(|w| w.what.starts_with("Reload"));
        if let Some(w) = line {
            eprintln!("  {:?} {}: +{:.2} kills, {:?} score. {}", w.group, w.what, w.kills, w.score, w.how);
        }
        // every measured kill carries its reloads; a kill with more shots than the magazine waited for one at least
        assert!(got.flicks.iter().all(|m| m.reloads.is_some() && m.reload_time.is_some()));
        assert!(got.flicks.iter().all(|m| m.shots.unwrap() <= rules.magazine || m.reloads.unwrap() >= 1));
        let measured: i64 = got.flicks.iter().map(|m| m.reloads.unwrap()).sum();
        assert!(measured <= reloads.count && reloads.count > 0, "{run}");
        // every kill took one hit: with no misses no reload is forced, so the line gives back all the reload time
        let kills = line.expect("a reload line").kills;
        assert!(kills > 0.0 && kills <= s.kills as f64, "{run}");
    }
}
