//! Labelling (python/server.py): the recordings the user marked as another game, the queue of recordings to label
//! areas in and the ones skipped there, kept in the data folder as the review server keeps them (not_aim_trainer.json,
//! label_skipped.json: sorted lists of recording ids). In: the page's marks and skips (/api/not_aim, /api/label_skip)
//! and the recordings list. Out: those files, and the queues the page labels from (/api/label_queue; faint.rs's).

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::{Value, json};

use crate::library::{Answer, Library};
use crate::pyjson;

const NOT_AIM: &str = "not_aim_trainer.json";
const LABEL_SKIPPED: &str = "label_skipped.json";

/// A list of recording ids kept in a file; none when it is missing.
pub(crate) fn read_ids(path: &Path) -> BTreeSet<String> {
    crate::disk::read(path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

/// Keeps the ids, sorted, as `json.dump(sorted(ids), indent=1)`.
pub(crate) fn write_ids(path: &Path, ids: &BTreeSet<String>) -> Answer<()> {
    Ok(pyjson::dump(path, &ids.iter().collect::<Vec<_>>(), true)?)
}

/// The id added to the list in a file: a skipped recording.
pub(crate) fn add_id(path: &Path, id: &str) -> Answer<Value> {
    let mut ids = read_ids(path);
    ids.insert(id.to_string());
    write_ids(path, &ids)?;
    Ok(json!({ "id": id, "skipped": true }))
}

/// Whether a recordings list's row is an upload.
fn is_uploaded_row(row: &Value) -> bool {
    row["uploaded"].as_bool() == Some(true)
}

impl Library {
    /// The recordings the user marked as another game, not an aim trainer.
    pub fn not_aim(&self) -> BTreeSet<String> {
        read_ids(&self.file(NOT_AIM))
    }

    /// Marks a recording as another game (on), or as an aim trainer again: a marked one is left out of the queues and
    /// of what the area finder learns.
    pub fn set_not_aim(&self, id: &str, on: bool) -> Answer<Value> {
        self.resolve(id)?;
        let mut ids = self.not_aim();
        if on {
            ids.insert(id.to_string());
        } else {
            ids.remove(id);
        }
        write_ids(&self.file(NOT_AIM), &ids)?;
        Ok(json!({ "id": id, "not_aim": on }))
    }

    /// Skipped in the labelling queue: left out of it from now on.
    pub fn skip_label(&self, id: &str) -> Answer<Value> {
        add_id(&self.file(LABEL_SKIPPED), id)
    }

    /// Recordings to label areas in: uploads first (other players' layouts), then the most recent recording of each
    /// scenario, leaving out probes, other games, skipped ones and those with saved areas.
    pub fn label_queue(&self) -> Answer<Value> {
        let skipped = read_ids(&self.file(LABEL_SKIPPED));
        let labelled = |id: &str| crate::disk::exists(self.review_dir(id).join("exclude.json"));
        let ids = self.queue(|id| skipped.contains(id) || labelled(id))?;
        Ok(json!(ids))
    }

    /// The labelling queues' order (python/server.py: label_queue, faint_queue): uploads first, then the rest, newest
    /// first; one recording per scenario folder (each upload is its own), none from a probe scenario or another game,
    /// and none that `left_out` leaves out.
    pub(crate) fn queue(&self, left_out: impl Fn(&str) -> bool) -> Answer<Vec<String>> {
        let Value::Array(mut list) = self.recordings(false)? else { return Ok(Vec::new()) };
        // a stable sort, as Python's sorted(): equal times keep the list's order
        list.sort_by(|a, b| {
            let newest_first = |row: &Value| -row["mtime"].as_f64().unwrap_or(0.0);
            (!is_uploaded_row(a)).cmp(&!is_uploaded_row(b)).then(newest_first(a).total_cmp(&newest_first(b)))
        });
        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        for row in &list {
            let id = row["id"].as_str().unwrap_or_default();
            let scenario = if is_uploaded_row(row) { id } else { id.split('/').next().unwrap_or(id) };
            let other_game = row["not_aim"].as_bool() == Some(true);
            if seen.contains(scenario) || scenario.contains("Probe") || other_game || left_out(id) {
                continue;
            }
            seen.insert(scenario.to_string());
            out.push(id.to_string());
        }
        Ok(out)
    }
}
