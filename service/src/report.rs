//! A review's report, worked out by the core's typed entry (src/review.rs: `review_request`; the browser sends the
//! same request as JSON to `review_json`) from what the review keeps (store.rs: `Part`): its tracks, readings and what
//! the HUD read (a review made before the HUD was read has none), each read once straight into its type. With a stats
//! file the core reviews from it; without one, from the HUD's reading, else from the video alone
//! (python/retired/server.py did the same). In: the review's parts and the recording's stats file, run marks, facts,
//! chosen hitbox and cut-off (library/reviews.rs; aimview-tool's from a folder, store.rs: `folder_parts`). Out: the
//! report's JSON, which /api/report answers.

use std::path::Path;

use aimview::camera::VideoReadings;
use aimview::faint::FaintSetting;
use aimview::hud::HudReading;
use aimview::kill_check::KillEvidence;
use aimview::review::{ReviewRequest, review_request};
use aimview::scenario::{Facts, Hitbox, HitboxKind, Kind};
use aimview::stats_file::StatsFile;
use aimview::track::Tracks;
use aimview::tracking::box_ratio;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::run_window::RunMarks;
use crate::store::Part;

/// The core's words for a request it cannot read (src/review.rs `review_json`), which a part that is not its type
/// gives too.
const UNREADABLE: &str = "The review request could not be read";

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

/// What a report is worked out from besides the review's parts and the video (Introduce Parameter Object), each None
/// where there is none: a caller names those it has and takes the rest from `ReportInputs::default()`.
#[derive(Default)]
pub struct ReportInputs<'a> {
    /// The stats file: its path and its text.
    pub stats: Option<(&'a Path, &'a [u8])>,
    /// The user's run marks.
    pub run: Option<RunMarks>,
    /// The scenario's facts.
    pub facts: Option<&'a Facts>,
    /// The bots' hitbox shape the user chose in place of the facts' hitbox (None: the facts'); its proportions come
    /// from the tracks (`chosen_hitbox`).
    pub hitbox_pick: Option<HitboxKind>,
    /// The user's faint-target cut-off (faint.json).
    pub faint: Option<FaintSetting>,
}

/// The review's parts, each read straight into its type.
struct ReviewParts {
    /// The run's tracks (tracks.json).
    tracks: Tracks,
    /// The camera's turn and the countdown bar per frame (readings.json); none where the review has none.
    readings: VideoReadings,
    /// What the HUD read (hud.json); None where it was not read.
    hud: Option<HudReading>,
    /// The check of the kills the video alone gives (kills.json); None where it was not checked.
    kill_check: Option<Vec<KillEvidence>>,
}

/// A part read straight into its type: None when it is missing or not JSON at all (the report goes without it, as it
/// always has); JSON that is not the part's type fails the report, as the core's reading of the request did.
fn read_part<T: DeserializeOwned>(part: Part, bytes: Option<Vec<u8>>) -> Result<Option<T>, String> {
    let Some(bytes) = bytes else { return Ok(None) };
    match serde_json::from_slice(&bytes) {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.is_data() => Err(format!("{UNREADABLE}: {}: {error}", part.file_name())),
        Err(_) => Ok(None),
    }
}

/// The review's parts that `parts` gives; None when it has no tracks.
fn read_parts(parts: impl Fn(Part) -> Option<Vec<u8>>) -> Result<Option<ReviewParts>, String> {
    let Some(tracks) = read_part(Part::Tracks, parts(Part::Tracks))? else { return Ok(None) };
    Ok(Some(ReviewParts {
        tracks,
        readings: read_part(Part::Readings, parts(Part::Readings))?.unwrap_or_default(),
        hud: read_part::<Option<HudReading>>(Part::Hud, parts(Part::Hud))?.flatten(),
        kill_check: read_part::<Option<Vec<KillEvidence>>>(Part::Kills, parts(Part::Kills))?.flatten(),
    }))
}

/// The hitbox the user chose (`pick`) for the report in place of the scenario's: a sphere is as wide as it is tall; a
/// capsule's or a box's width over height is the review's target boxes' (1 without them).
pub fn chosen_hitbox(pick: HitboxKind, tracks: &Tracks) -> Hitbox {
    let width_to_height = match pick {
        HitboxKind::Spheroid => 1.0,
        HitboxKind::Cylindrical | HitboxKind::Cuboid => box_ratio(&tracks.frames).unwrap_or(1.0),
    };
    Hitbox { kind: pick, width_to_height }
}

/// The core's review request for `video` from its parts and `inputs`.
fn request(parts: ReviewParts, video: &Path, inputs: ReportInputs<'_>) -> Result<ReviewRequest, String> {
    let ReportInputs { stats, run, facts, hitbox_pick, faint } = inputs;
    let ReviewParts { tracks, readings, hud, kill_check } = parts;
    let stats_text = stats.map_or_else(String::new, |(_, bytes)| String::from_utf8_lossy(bytes).into_owned());
    let picked = hitbox_pick.map(|pick| chosen_hitbox(pick, &tracks));
    let run = run.filter(RunMarks::is_set).map(serde_json::to_value).transpose().map_err(|error| error.to_string())?;
    Ok(ReviewRequest {
        video: file_name(video).ok_or_else(|| format!("{UNREADABLE}: {} has no file name", video.display()))?,
        stats: stats.and_then(|(path, _)| file_name(path)).unwrap_or_default(),
        tracking: is_tracking(facts, &stats_text),
        stats_text,
        tracks,
        hud,
        run,
        limit: facts.and_then(|facts| facts.limit),
        reload: facts.and_then(|facts| facts.reload.clone()),
        camera: readings.camera,
        countdown: readings.countdown,
        faint,
        hitbox: picked.or_else(|| facts.and_then(|facts| facts.hitbox)),
        kill_check,
    })
}

/// The report of the review of `video` whose parts `parts` gives, worked out with `inputs` by the core's typed entry
/// (src/review.rs `review_request`) and made JSON once; None when the review has no tracks.
pub fn work_out(
    parts: impl Fn(Part) -> Option<Vec<u8>>,
    video: &Path,
    inputs: ReportInputs<'_>,
) -> Result<Option<Value>, String> {
    let Some(parts) = read_parts(parts)? else { return Ok(None) };
    let report = review_request(request(parts, video, inputs)?)?;
    serde_json::to_value(&report).map(Some).map_err(|error| error.to_string())
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
