//! A review's report, worked out by the core as the browser does (src/review.rs: `review_json`), from what the review
//! keeps (store.rs: `Part`): its tracks, readings and what the HUD read (a review made before the HUD was read has
//! none). With a stats file the core reviews from it; without one, from the HUD's reading, else from the video alone
//! (python/retired/server.py did the same). In: the review's parts and the recording's stats file, run marks, facts
//! and cut-off (library/reviews.rs; aimview-tool's from a folder, store.rs: `folder_parts`). Out: the report's JSON,
//! which /api/report answers.

use std::path::Path;

use aimview::scenario::{Facts, Hitbox, Kind};
use aimview::stats_file::StatsFile;
use serde_json::{Value, json};

use crate::run_window::RunMarks;
use crate::store::Part;

/// Whether the run was a tracking run: by its scenario's facts, else (its scenario file not here: in the browser, KovaaK's
/// stats folder chosen without the scenarios) by its stats file, which for a pure tracking scenario counts no kills and
/// some hits. A tracking scenario whose bots die needs its scenario file to be known as one.
fn is_tracking(facts: Option<&Facts>, stats_text: &str) -> bool {
    if let Some(facts) = facts {
        return facts.kind == Kind::Tracking;
    }
    let meta = StatsFile::parse(stats_text).meta;
    let number = |key: &str| meta.get(key).and_then(|value| value.trim().parse::<f64>().ok());
    number("Kills") == Some(0.0) && number("Hit Count").is_some_and(|hits| hits > 0.0)
}

/// A file's name, as the core's review request takes it.
fn file_name(path: &Path) -> Option<String> {
    path.file_name().map(|name| name.to_string_lossy().into_owned())
}

/// The report of the review of `video` whose parts `parts` gives, with its stats file (its path and text) when it has
/// one, the user's run
/// marks, the scenario's facts, the bots' hitbox the user chose in place of the facts' (None: the facts') and the
/// user's faint-target cut-off (faint.json: {on, offset}); None when the review has
/// no tracks.
pub fn work_out(
    parts: impl Fn(Part) -> Option<Vec<u8>>,
    video: &Path,
    stats: Option<(&Path, &[u8])>,
    run: Option<RunMarks>,
    facts: Option<&Facts>,
    hitbox: Option<Hitbox>,
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
        "tracking": is_tracking(facts, &stats_text),
        "limit": facts.and_then(|facts| facts.limit),
        "reload": facts.and_then(|facts| facts.reload.as_ref()),
        "hitbox": hitbox.or_else(|| facts.and_then(|facts| facts.hitbox)),
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

/// The run's kind when its scenario file is missing.
#[cfg(test)]
mod tests {
    use super::*;

    /// A stats file's end: its kills and hits.
    fn stats(kills: u32, hits: u32) -> String {
        format!(
            "Kills:,{kills}
Deaths:,0
Hit Count:,{hits}
Miss Count:,10
Score:,{hits}.0
"
        )
    }

    /// Without facts, no kills and some hits is tracking; kills, or no hits, is not; facts always win.
    #[test]
    fn tells_tracking_without_the_scenario_file() {
        assert!(is_tracking(None, &stats(0, 4663)));
        assert!(!is_tracking(None, &stats(7, 4663)), "bots died: not known as tracking without the file");
        assert!(!is_tracking(None, &stats(0, 0)), "no hits at all");
        assert!(!is_tracking(None, ""), "no stats file");
        let clicking = Facts { kind: Kind::Static, limit: None, targets: None, reload: None, hitbox: None };
        assert!(!is_tracking(Some(&clicking), &stats(0, 4663)), "the scenario's facts win");
        let tracking = Facts { kind: Kind::Tracking, ..clicking };
        assert!(is_tracking(Some(&tracking), &stats(7, 4663)));
    }
}
