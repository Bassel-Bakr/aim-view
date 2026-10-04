//! The review's last steps, from the joined tracks: the kills matched in the tracks, each flick measured, and the
//! report worked out (the service's report request).

use std::hint::black_box;

use aimview::hud::HudReading;
use aimview::matching::{match_times, match_video};
use aimview::measure::{measure as measure_flicks, target_radius};
use aimview::review::{KillTimes, VideoReadings, review_clicks, review_json, review_tracking};
use aimview::stats_file::StatsFile;
use aimview::track::Tracks;
use aimview::tracking::CameraReading;
use criterion::{Criterion, SamplingMode};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::inputs::{self, AV1, FLOWER, NATIVE};

/// av1's stats file, as the byte-compare's review takes it.
const AV1_STATS: &str = "test_out/parity/av1/review/stats.csv";
const FLOWER_STATS: &str = "test_out/parity/flower/review/stats.csv";

/// readings.json: each frame's camera reading and whether KovaaK's countdown bar shows.
#[derive(Deserialize)]
struct Readings {
    camera: Vec<CameraReading>,
    countdown: Vec<bool>,
}

/// av1's tracks from the native review and its stats file's text.
fn av1(bench: &str) -> Option<(Tracks, String)> {
    let tracks = inputs::json(bench, &format!("{NATIVE}/{AV1}/tracks.json"))?;
    Some((tracks, inputs::text(bench, AV1_STATS)?))
}

pub fn matching(c: &mut Criterion) {
    let Some((tracks, stats)) = av1("matching") else { return };
    let kills = StatsFile::parse(&stats).kills().unwrap();
    let mut g = c.benchmark_group("matching");
    g.sample_size(20).sampling_mode(SamplingMode::Flat);
    g.bench_function("match_times_av1", |b| {
        b.iter(|| match_times(black_box(&tracks), &kills.times, &kills.shots, 0.25, None))
    });
    // the kills from the video alone (no stats file, no HUD)
    g.bench_function("match_video_av1", |b| b.iter(|| match_video(black_box(&tracks))));
    g.finish();
}

pub fn measure(c: &mut Criterion) {
    let Some((tracks, stats)) = av1("measure") else { return };
    let kills = StatsFile::parse(&stats).kills().unwrap();
    let (flicks, _) = match_times(&tracks, &kills.times, &kills.shots, 0.25, None);
    let r = target_radius(&flicks);
    let mut g = c.benchmark_group("measure");
    g.sample_size(20);
    g.bench_function("measure_av1", |b| b.iter(|| measure_flicks(black_box(&flicks), &tracks, r)));
    g.finish();
}

pub fn report(c: &mut Criterion) {
    let mut g = c.benchmark_group("report");
    g.sample_size(10).sampling_mode(SamplingMode::Flat);
    let video = format!("{AV1}.mp4");
    if let Some((tracks, stats)) = av1("report/clicks_av1") {
        let kills = KillTimes::Stats { name: "stats.csv", text: &stats };
        g.bench_function("clicks_av1", |b| b.iter(|| review_clicks(black_box(&tracks), kills, &video, None, None)));
    }
    // without the stats file: the kills from the HUD the review read
    let dir = format!("{NATIVE}/{AV1}/no_stats");
    let tracks: Option<Tracks> = inputs::json("report/hud_av1", &format!("{dir}/tracks.json"));
    let hud: Option<Option<HudReading>> = inputs::json("report/hud_av1", &format!("{dir}/hud.json"));
    if let (Some(tracks), Some(hud)) = (tracks, hud) {
        let kills = KillTimes::Unpaired { hud: hud.as_ref() };
        g.bench_function("hud_av1", |b| b.iter(|| review_clicks(black_box(&tracks), kills, &video, None, None)));
    }
    // the service's request (service/src/report.rs, `work_out`), JSON in and out, as the byte-compare's report
    let dir = format!("{NATIVE}/{AV1}");
    let tracks: Option<Value> = inputs::json("report/json_av1", &format!("{dir}/tracks.json"));
    let readings: Option<Value> = inputs::json("report/json_av1", &format!("{dir}/readings.json"));
    let hud: Option<Value> = inputs::json("report/json_av1", &format!("{dir}/hud.json"));
    let stats = inputs::text("report/json_av1", AV1_STATS);
    if let (Some(tracks), Some(readings), Some(hud), Some(stats)) = (tracks, readings, hud, stats) {
        let request = serde_json::to_vec(&json!({
            "tracks": tracks, "statsText": stats, "video": video, "stats": "stats.csv", "hud": hud, "run": null,
            "tracking": false, "limit": null, "reload": null, "camera": readings["camera"],
            "countdown": readings["countdown"], "faint": null,
        }))
        .unwrap();
        g.bench_function("json_av1", |b| b.iter(|| review_json(black_box(&request))));
    }
    // a tracking run: its time on the target from the tracks and the camera's readings
    let bench = "report/tracking_flower";
    let dir = format!("{NATIVE}/{FLOWER}");
    let tracks: Option<Tracks> = inputs::json(bench, &format!("{dir}/tracks.json"));
    let readings: Option<Readings> = inputs::json(bench, &format!("{dir}/readings.json"));
    let stats = inputs::text(bench, FLOWER_STATS);
    let facts: Option<Value> = inputs::json(bench, "test_out/parity/scenarios.json");
    if let (Some(tracks), Some(readings), Some(stats), Some(facts)) = (tracks, readings, stats, facts) {
        let limit = facts["facts"]["flower easier"]["limit"].as_f64();
        let video = format!("{FLOWER}.mp4");
        g.bench_function("tracking_flower", |b| {
            b.iter(|| {
                let kills = KillTimes::Stats { name: "stats.csv", text: &stats };
                let r = VideoReadings { camera: &readings.camera, countdown: &readings.countdown };
                review_tracking(black_box(&tracks), kills, &video, limit, r, None, None)
            })
        });
    }
    g.finish();
}
