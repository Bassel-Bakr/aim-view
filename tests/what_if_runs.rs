//! The what-if lines (src/what_if.rs) on real runs with their stats files: the parity runs (test_out/parity/<case>/
//! review/) and some of the video-alone benchmark's (test_out/vod_model/eval/video_alone/full_v3/<run>/). Each line must
//! be plausible: at least half a kill, no more than the run's kills, biggest first. `--nocapture` prints them.

use std::fs;
use std::path::{Path, PathBuf};

use aimview::review::{KillTimes, review_clicks};
use aimview::track::Tracks;

const STATS: &str = r"C:\Program Files (x86)\Steam\steamapps\common\FPSAimTrainer\FPSAimTrainer\stats";
const RUNS: [&str; 5] = [
    "parity/av1/review",
    "parity/pokeball134/review",
    "vod_model/eval/video_alone/full_v3/1wall_6targets_extra_small_-_849.91_-_2026.10.01-16.15.22",
    "vod_model/eval/video_alone/full_v3/1w4ts_Voltaic_-_143_-_2026.09.30-04.55.23",
    "vod_model/eval/video_alone/full_v3/Pasu_Switch_Wide_-_460_-_2026.07.19-16.07.10",
];

#[test]
fn what_if_lines_are_plausible_on_real_runs() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_out");
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
        let path = if Path::new(stats).is_absolute() { PathBuf::from(stats) } else { Path::new(STATS).join(stats) };
        let Ok(text) = fs::read(&path).map(|b| String::from_utf8_lossy(&b).into_owned()) else {
            eprintln!("no {}", path.display());
            continue;
        };
        let got = review_clicks(&tracks, KillTimes::Stats { name: stats, text: &text }, report["video"].as_str().unwrap(), None)
            .unwrap()
            .report;
        let s = &got.summary;
        eprintln!("{} ({:?}, {} kills, {} measured, score {:?})", s.scenario.as_deref().unwrap_or(run), s.mode, s.kills, s.measured, s.score);
        for w in &s.what_if {
            eprintln!("  {:?} {}: +{:.1} kills, {:?} score. {}", w.group, w.what, w.kills, w.score.map(|v| (v * 10.0).round() / 10.0), w.how);
            assert!(w.kills >= 0.5 && w.kills <= s.kills as f64 && w.kills <= s.measured as f64, "{}: {}", run, w.what);
        }
        assert!(s.what_if.windows(2).all(|p| p[0].kills >= p[1].kills), "{run}: not biggest first");
    }
}
