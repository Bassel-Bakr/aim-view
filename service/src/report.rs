//! A review's report, worked out by the core as the browser does (src/review.rs: `review_json`), from what the review
//! keeps (store.rs: `Part`): its tracks, readings and what the HUD read (a review made before the HUD was read has
//! none). With a stats file the core reviews from it; without one, from the HUD's reading, else from the video alone
//! (python/retired/server.py did the same). In: the review's parts and the recording's stats file, run marks, facts
//! and cut-off (library/reviews.rs; aimview-tool's from a folder, store.rs: `folder_parts`). Out: the report's JSON,
//! which /api/report answers.

use std::path::Path;

use aimview::scenario::{Facts, Kind};
use serde_json::{Value, json};

use crate::run_window::RunMarks;
use crate::store::Part;

/// A file's name, as the core's review request takes it.
fn file_name(path: &Path) -> Option<String> {
    path.file_name().map(|name| name.to_string_lossy().into_owned())
}

/// The report of the review of `video` whose parts `parts` gives, with its stats file (its path and text) when it has
/// one, the user's run
/// marks, the scenario's facts and the user's faint-target cut-off (faint.json: {on, offset}); None when the review has
/// no tracks.
pub fn work_out(
    parts: impl Fn(Part) -> Option<Vec<u8>>,
    video: &Path,
    stats: Option<(&Path, &[u8])>,
    run: Option<RunMarks>,
    facts: Option<&Facts>,
    faint: Option<Value>,
) -> Result<Option<Value>, String> {
    // each part's JSON, or None when it is missing or not JSON
    let read = |part: Part| serde_json::from_slice::<Value>(&parts(part)?).ok();
    let Some(tracks) = read(Part::Tracks) else { return Ok(None) };
    let readings = read(Part::Readings).unwrap_or(json!({ "camera": [], "countdown": [] }));
    let hud = read(Part::Hud).unwrap_or(Value::Null);
    // the check of the kills the video alone gives (null or missing: not checked)
    let kill_check = read(Part::Kills).unwrap_or(Value::Null);
    let stats_text = stats.map_or_else(String::new, |(_, bytes)| String::from_utf8_lossy(bytes).into_owned());
    let request = json!({
        "tracks": tracks,
        "statsText": stats_text,
        "video": file_name(video),
        "stats": stats.and_then(|(path, _)| file_name(path)).unwrap_or_default(),
        "hud": hud,
        "run": run.filter(RunMarks::is_set),
        "tracking": facts.is_some_and(|facts| facts.kind == Kind::Tracking),
        "limit": facts.and_then(|facts| facts.limit),
        "reload": facts.and_then(|facts| facts.reload.as_ref()),
        "hitbox": facts.and_then(|facts| facts.hitbox),
        "killCheck": kill_check,
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
