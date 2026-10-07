//! The what-if lines (src/what_if.rs) on real runs with their stats files: the parity runs (test_out/parity/<case>/
//! review/) and some of the video-alone benchmark's (test_out/vod_model/eval/video_alone/full_v3/<run>/). Each line
//! must be plausible: at least half a kill, no more than the run's kills, biggest first. `--nocapture` prints them.

use std::fs;
use std::path::{Path, PathBuf};

use aimview::local_config::LocalConfig;
use aimview::review::{KillTimes, Report, review_clicks};
use aimview::track::Tracks;

const RUNS: [&str; 5] = [
    "parity/av1/review",
    "parity/pokeball134/review",
    "vod_model/eval/video_alone/full_v3/1wall_6targets_extra_small_-_849.91_-_2026.10.01-16.15.22",
    "vod_model/eval/video_alone/full_v3/1w4ts_Voltaic_-_143_-_2026.09.30-04.55.23",
    "vod_model/eval/video_alone/full_v3/Pasu_Switch_Wide_-_460_-_2026.07.19-16.07.10",
];
/// The fewest kills a what-if line may add.
const MIN_LINE_KILLS: f64 = 0.5;

/// Prints the run's what-if lines and checks each is plausible, biggest first.
fn check_lines(run: &str, report: &Report) {
    let summary = &report.summary;
    let scenario = summary.scenario.as_deref().unwrap_or(run);
    let (mode, kills, measured, score) = (&summary.mode, summary.kills, summary.measured, summary.score);
    eprintln!("{scenario} ({mode:?}, {kills} kills, {measured} measured, score {score:?})");
    for line in &summary.what_if {
        let score = line.score.map(|value| (value * 10.0).round() / 10.0);
        eprintln!("  {:?} {}: +{:.1} kills, {score:?} score. {}", line.group, line.what, line.kills, line.how);
        let plausible = line.kills >= MIN_LINE_KILLS && line.kills <= kills as f64 && line.kills <= measured as f64;
        assert!(plausible, "{}: {}", run, line.what);
    }
    assert!(summary.what_if.windows(2).all(|pair| pair[0].kills >= pair[1].kills), "{run}: not biggest first");
}

#[test]
fn what_if_lines_are_plausible_on_real_runs() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_out");
    let stats_folder = LocalConfig::load().kovaak("stats").unwrap_or_default();
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
        let path = if Path::new(stats).is_absolute() { PathBuf::from(stats) } else { stats_folder.join(stats) };
        let Ok(text) = fs::read(&path).map(|bytes| String::from_utf8_lossy(&bytes).into_owned()) else {
            eprintln!("no {}", path.display());
            continue;
        };
        let kills = KillTimes::Stats { name: stats, text: &text };
        let got = review_clicks(&tracks, kills, report["video"].as_str().unwrap(), None, None).unwrap().report;
        check_lines(run, &got);
    }
}
