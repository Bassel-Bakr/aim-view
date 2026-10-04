//! A review's report, worked out by the core as the browser does (src/review.rs: `review_json`), from what the review
//! keeps in its folder: tracks.json, readings.json and hud.json (what the HUD read; a review made before the HUD was
//! read has none). With a stats file the core reviews from it; without one, from the HUD's reading, else from the video
//! alone (python/server.py does the same). In: the review's folder and the recording's stats file, run marks, facts
//! and cut-off (library/reviews.rs, aimview-tool). Out: the report's JSON, which /api/report answers.

use std::path::Path;

use aimview::scenario::{Facts, Kind};
use serde_json::{Value, json};

use crate::run_window::RunMarks;

/// A JSON file, or None when it is missing or not JSON.
fn read(path: &Path) -> Option<Value> {
    serde_json::from_slice(&crate::disk::read(path).ok()?).ok()
}

/// A file's name, as the core's review request takes it.
fn file_name(path: &Path) -> Option<String> {
    path.file_name().map(|name| name.to_string_lossy().into_owned())
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
        Some(path) => {
            let bytes = crate::disk::read(path).map_err(|error| error.to_string())?;
            String::from_utf8_lossy(&bytes).into_owned()
        }
        None => String::new(),
    };
    let request = json!({
        "tracks": tracks,
        "statsText": stats_text,
        "video": file_name(video),
        "stats": stats.and_then(file_name).unwrap_or_default(),
        "hud": hud,
        "run": run.filter(RunMarks::is_set),
        "tracking": facts.is_some_and(|facts| facts.kind == Kind::Tracking),
        "limit": facts.and_then(|facts| facts.limit),
        "reload": facts.and_then(|facts| facts.reload.as_ref()),
        "camera": readings["camera"],
        "countdown": readings["countdown"],
        "faint": faint,
    });
    let request = serde_json::to_vec(&request).map_err(|error| error.to_string())?;
    let outcome: Value =
        serde_json::from_slice(&aimview::review::review_json(&request)).map_err(|error| error.to_string())?;
    if let Some(error) = outcome["error"].as_str() {
        return Err(error.to_string());
    }
    Ok(Some(outcome["report"].clone()))
}
