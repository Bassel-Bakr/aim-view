//! A review's report, worked out by the core as the browser does (src/review.rs: `review_json`), from what the review
//! keeps in its folder: tracks.json, readings.json and hud.json (what the HUD read; a review made before the HUD was
//! read has none). With a stats file the core reviews from it; without one, from the HUD's reading, else from the video
//! alone (python/server.py does the same).

use std::path::Path;

use aimview::scenario::{Facts, Kind};
use serde_json::{Value, json};

use crate::run_window::RunMarks;

fn read(p: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(p).ok()?).ok()
}

/// The report of the review in `dir` of `video`, with its stats file when it has one, the user's run marks, the
/// scenario's facts and the user's faint-target cut-off (faint.json: {on, offset}); None when the folder has no tracks.
pub fn work_out(
    dir: &Path,
    video: &Path,
    stats: Option<&Path>,
    run: Option<RunMarks>,
    facts: Option<&Facts>,
    faint: Option<Value>,
) -> Result<Option<Value>, String> {
    let Some(tracks) = read(&dir.join("tracks.json")) else { return Ok(None) };
    let readings = read(&dir.join("readings.json")).unwrap_or(json!({ "camera": [], "countdown": [] }));
    let hud = read(&dir.join("hud.json")).unwrap_or(Value::Null);
    let stats_text = match stats {
        Some(p) => String::from_utf8_lossy(&std::fs::read(p).map_err(|e| e.to_string())?).into_owned(),
        None => String::new(),
    };
    let name = |p: &Path| p.file_name().map(|n| n.to_string_lossy().into_owned());
    let request = json!({
        "tracks": tracks,
        "statsText": stats_text,
        "video": name(video),
        "stats": stats.and_then(name).unwrap_or_default(),
        "hud": hud,
        "run": run.filter(RunMarks::is_set),
        "tracking": facts.is_some_and(|f| f.kind == Kind::Tracking),
        "limit": facts.and_then(|f| f.limit),
        "camera": readings["camera"],
        "countdown": readings["countdown"],
        "faint": faint,
    });
    let outcome: Value = serde_json::from_slice(&aimview::review::review_json(&serde_json::to_vec(&request).map_err(|e| e.to_string())?))
        .map_err(|e| e.to_string())?;
    if let Some(e) = outcome["error"].as_str() {
        return Err(e.to_string());
    }
    Ok(Some(outcome["report"].clone()))
}
