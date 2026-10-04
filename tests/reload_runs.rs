//! The forced reloads (src/reload.rs) on real runs of scenarios whose magazine runs out, reviewed with their stats
//! files into test_out/reload_runs/<run>/ (`aimview-tool review <video> --out <that folder> --stats-file <csv>`), with
//! the ammo rules read from the scenario's file. `--nocapture` prints each run's reloads and its what-if line.

use std::fs;
use std::path::{Path, PathBuf};

use aimview::review::{KillTimes, Report, review_clicks};
use aimview::scenario::{AmmoRules, facts};
use aimview::track::Tracks;

const STEAMAPPS: &str = r"C:\Program Files (x86)\Steam\steamapps";
const SCENARIOS: &str = r"common\FPSAimTrainer\FPSAimTrainer\Saved\SaveGames\Scenarios";
const WORKSHOP: &str = r"workshop\content\824270";
const STATS: &str = r"common\FPSAimTrainer\FPSAimTrainer\stats";
/// What separates the scenario's name from the rest in a stats file's name.
const STATS_NAME_SEPARATOR: &str = " - Challenge - ";
const RUNS: [&str; 1] = ["Pasu_Reload_Goated_-_112_-_2026.08.24-02.21.32"];

/// The scenario's file: the user's own, else the workshop's.
fn scenario_file(name: &str) -> Option<PathBuf> {
    let own = Path::new(STEAMAPPS).join(SCENARIOS).join(format!("{name}.sce"));
    if own.exists() {
        return Some(own);
    }
    fs::read_dir(Path::new(STEAMAPPS).join(WORKSHOP))
        .ok()?
        .flatten()
        .map(|item| item.path().join(format!("{name}.sce")))
        .find(|path| path.exists())
}

/// Prints the run's ammo rules, its forced reloads, the kills that waited for one and its what-if line.
fn print_reloads(scenario: &str, rules: &AmmoRules, report: &Report) {
    let summary = &report.summary;
    eprintln!("{scenario} ({} kills, {} measured): {rules:?}", summary.kills, summary.measured);
    eprintln!("  forced reloads: {:?}", summary.reloads.as_ref().unwrap());
    for flick in report.flicks.iter().filter(|flick| flick.reloads.is_some_and(|count| count > 0)) {
        let (kill, shots, reloads, time) = (flick.kill_number, flick.shots, flick.reloads, flick.reload_time);
        eprintln!("  kill {kill}: {shots:?} shots, {reloads:?} reloads, {time:?} s");
    }
    if let Some(line) = summary.what_if.iter().find(|line| line.what.starts_with("Reload")) {
        let (group, what, kills, score, how) = (&line.group, &line.what, line.kills, line.score, &line.how);
        eprintln!("  {group:?} {what}: +{kills:.2} kills, {score:?} score. {how}");
    }
}

/// Every measured kill carries its reloads, and no more of them than the run's forced reloads; a kill with more shots
/// than the magazine waited for one at least; the what-if line gives back some kills, no more than the run's.
fn check_reloads(run: &str, rules: &AmmoRules, report: &Report) {
    let summary = &report.summary;
    let reloads = summary.reloads.as_ref().unwrap();
    let flicks = &report.flicks;
    assert!(flicks.iter().all(|flick| flick.reloads.is_some() && flick.reload_time.is_some()));
    assert!(flicks.iter().all(|flick| flick.shots.unwrap() <= rules.magazine || flick.reloads.unwrap() >= 1));
    let measured: i64 = flicks.iter().map(|flick| flick.reloads.unwrap()).sum();
    assert!(measured <= reloads.count && reloads.count > 0, "{run}");
    // every kill took one hit: with no misses no reload is forced, so the line gives back all the reload time
    let line = summary.what_if.iter().find(|line| line.what.starts_with("Reload"));
    let kills = line.expect("a reload line").kills;
    assert!(kills > 0.0 && kills <= summary.kills as f64, "{run}");
}

#[test]
fn forced_reloads_on_real_runs() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_out/reload_runs");
    for run in RUNS {
        let dir = root.join(run);
        let (Ok(tracks), Ok(report)) =
            (fs::read_to_string(dir.join("tracks.json")), fs::read_to_string(dir.join("report.json")))
        else {
            eprintln!("no {}", dir.display());
            continue;
        };
        let tracks: Tracks = serde_json::from_str(&tracks).unwrap();
        let report: serde_json::Value = serde_json::from_str(&report).unwrap();
        let stats = report["stats"].as_str().unwrap();
        let path = Path::new(STEAMAPPS).join(STATS).join(stats);
        let scenario = stats.split(STATS_NAME_SEPARATOR).next().unwrap();
        let stats_text = fs::read(&path).map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
        let (Ok(text), Some(scenario_path)) = (stats_text, scenario_file(scenario)) else {
            eprintln!("no stats file or scenario file for {run}");
            continue;
        };
        let scenario_text = String::from_utf8_lossy(&fs::read(scenario_path).unwrap()).into_owned();
        let rules = facts(&scenario_text).reload.expect("a magazine that runs out");
        let kills = KillTimes::Stats { name: stats, text: &text };
        let video = report["video"].as_str().unwrap();
        let got = review_clicks(&tracks, kills, video, None, Some(&rules)).unwrap().report;
        print_reloads(scenario, &rules, &got);
        check_reloads(run, &rules, &got);
    }
}
