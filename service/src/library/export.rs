//! Recordings shared as one zip (docs/storage-design.md, "Export"): what the service keeps of each, for the page to
//! write into the zip beside the videos, and what the page gives back when it opens one. The same in every mode.
//!
//! An export's zip: manifest.json (format, app version, when, the mode, each recording), then a folder per recording,
//! recordings/<nn> <slug>/, holding reviews/<model>/<part>.json (the old review's model is `OLD_REVIEW`), marks/<file>
//! (the run window, cut-off and areas; not the stats pick, which names a file of this computer), stats/<name>.csv
//! (its stats file) and, when the user included it, video/<name> (the page adds it). In: POST /api/export {ids, mode}
//! and POST /api/import?id= (a recording's files, after the page added its video and stats file as uploads). Out: the
//! export's files as a batch (batch.rs), and an opened recording's reviews, marks and scenario facts kept.

use std::collections::HashMap;

use aimview::scenario::Facts;
use serde::Deserialize;
use serde_json::{Value, json};

use super::names::{local_stamp, parse_video, slug};
use super::{Answer, Failure, Library, keep_json, read_kept};
use crate::store::{Item, MARKS, Mark, PARTS, ReviewBy};

/// The export's layout; an opened export of another format is refused.
const FORMAT: u32 = 1;
/// The model folder of a recording's old review (from before reviews were kept per model): no model is named so.
const OLD_REVIEW: &str = "_old";
/// The manifest's file in the zip.
const MANIFEST: &str = "manifest.json";
/// A recording's scenario facts in the files the page sends back (POST /api/import).
const FACTS_FILE: &str = "facts.json";
/// The folders of a recording's files in the zip.
const REVIEWS: &str = "reviews/";
/// The marks' folder.
const MARKS_FOLDER: &str = "marks/";
/// The stats file's folder.
const STATS_FOLDER: &str = "stats/";

/// POST /api/export's body: the recordings, in the order they go in the zip, and the mode that made it.
#[derive(Deserialize)]
struct ExportAsk {
    /// The recordings' ids.
    ids: Vec<String>,
    /// "browser", "server" or "desktop", for the manifest.
    #[serde(default)]
    mode: String,
}

/// An opened recording's scenario and its facts (facts.json in POST /api/import).
#[derive(Deserialize)]
struct ImportedFacts {
    /// The scenario's name.
    scenario: String,
    /// Its facts.
    facts: Facts,
}

/// The marks an export carries: every mark but the stats pick.
fn exported_marks() -> impl Iterator<Item = Mark> {
    MARKS.into_iter().filter(|mark| !matches!(mark, Mark::StatsPick))
}

/// Whether `name` is one plain file or folder name: no separator, not "." or "..", not empty.
fn plain_name(name: &str) -> bool {
    !name.is_empty() && !name.contains(['/', '\\']) && name != "." && name != ".."
}

impl Library {
    /// One recording's files for the zip, under `folder`, and its manifest entry.
    fn export_recording(&self, id: &str, folder: &str, out: &mut Vec<u8>) -> Answer<Value> {
        let video = self.resolve(id)?;
        let now = crate::disk::now();
        let mut reviews = self.store().models(id);
        if self.store().old_review(id).is_some() {
            reviews.push(OLD_REVIEW.to_string());
        }
        for model in &reviews {
            let by = if model == OLD_REVIEW { ReviewBy::Old } else { ReviewBy::Model(model.clone()) };
            for part in PARTS {
                if let Some(bytes) = self.store().read(Item::ReviewPart(id, &by, part)).ok().flatten() {
                    crate::batch::push(out, &format!("{folder}/{REVIEWS}{model}/{}", part.file_name()), now, &bytes);
                }
            }
        }
        for mark in exported_marks() {
            if let Some(bytes) = self.store().read(Item::Mark(id, mark)).ok().flatten() {
                crate::batch::push(out, &format!("{folder}/{MARKS_FOLDER}{}", mark.file_name()), now, &bytes);
            }
        }
        let stats = self.stats_of(id, &video);
        let stats_name =
            stats.as_ref().and_then(|path| path.file_name()).map(|name| name.to_string_lossy().into_owned());
        if let (Some(path), Some(name)) = (&stats, &stats_name) {
            crate::batch::push(out, &format!("{folder}/{STATS_FOLDER}{name}"), now, &self.stats_bytes(path)?);
        }
        let video_name = video.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let scenario = parse_video(&video).map_or_else(
            || video.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default(),
            |(scenario, _, _)| scenario,
        );
        Ok(json!({
            "id": id, "folder": folder, "video": video_name, "scenario": scenario, "facts": self.facts_of(&video),
            "stats": stats_name, "reviews": reviews, "shown": self.shown(id).0,
        }))
    }

    /// POST /api/export: the recordings' files for the zip, as a batch (batch.rs), manifest.json last (each
    /// recording's entry says which files and which video it has; the page adds the videos). A recording that is not
    /// there fails the export (404).
    pub fn export(&self, body: &[u8]) -> Answer<Vec<u8>> {
        let ask: ExportAsk = serde_json::from_slice(body).map_err(|error| Failure::bad(error.to_string()))?;
        let mut out = Vec::new();
        let mut recordings = Vec::with_capacity(ask.ids.len());
        for (i, id) in ask.ids.iter().enumerate() {
            let folder = format!("recordings/{:02} {}", i + 1, slug(id));
            recordings.push(self.export_recording(id, &folder, &mut out)?);
        }
        let manifest = json!({
            "format": FORMAT, "app": env!("CARGO_PKG_VERSION"), "made": local_stamp(crate::disk::now()),
            "mode": ask.mode, "recordings": recordings,
        });
        let manifest = serde_json::to_vec_pretty(&manifest)?;
        crate::batch::push(&mut out, MANIFEST, crate::disk::now(), &manifest);
        Ok(out)
    }

    /// POST /api/import?id=: an opened export's recording, whose video and stats file the page has added as uploads
    /// (`id` is its upload): its reviews (reviews/<model>/<part>), its marks (marks/<file>) and its scenario's facts
    /// (facts.json), which are kept for a scenario this computer has no file of. A path of another kind is refused
    /// (400) before anything is kept. Answers how many reviews and marks it kept.
    pub fn import_recording(&self, id: &str, body: &[u8]) -> Answer<Value> {
        self.resolve(id)?;
        let files = crate::batch::read(body)?;
        let refused = |path: &str| Failure::bad(format!("not part of an exported recording: {path}"));
        let mut writes: Vec<(Item<'_>, &[u8])> = Vec::new();
        let review_by = |model: &str| if model == OLD_REVIEW { ReviewBy::Old } else { ReviewBy::Model(model.into()) };
        let models: Vec<(String, ReviewBy)> = files
            .iter()
            .filter_map(|file| file.path.strip_prefix(REVIEWS)?.split_once('/'))
            .map(|(model, _)| (model.to_string(), review_by(model)))
            .collect();
        let mut facts = None;
        for file in &files {
            if let Some((model, part_file)) = file.path.strip_prefix(REVIEWS).and_then(|rest| rest.split_once('/')) {
                let part = PARTS.into_iter().find(|part| part.file_name() == part_file);
                let part = part.filter(|_| plain_name(model)).ok_or_else(|| refused(file.path))?;
                let by = &models.iter().find(|(name, _)| name == model).ok_or_else(|| refused(file.path))?.1;
                writes.push((Item::ReviewPart(id, by, part), file.bytes));
            } else if let Some(name) = file.path.strip_prefix(MARKS_FOLDER) {
                let mark = exported_marks().find(|mark| mark.file_name() == name).ok_or_else(|| refused(file.path))?;
                writes.push((Item::Mark(id, mark), file.bytes));
            } else if file.path == FACTS_FILE {
                facts = Some(serde_json::from_slice::<ImportedFacts>(file.bytes).map_err(|_| refused(file.path))?);
            } else {
                return Err(refused(file.path));
            }
        }
        let (mut reviews, mut marks) = (0, 0);
        for (item, bytes) in &writes {
            self.store().write(*item, bytes).map_err(|error| format!("{}: {error}", self.store().name(*item)))?;
            if matches!(item, Item::ReviewPart(..)) {
                reviews += 1;
            } else {
                marks += 1;
            }
        }
        if let Some(imported) = facts {
            let mut kept: HashMap<String, Facts> = read_kept(self.store(), Item::ImportedScenarios).unwrap_or_default();
            kept.insert(imported.scenario.to_lowercase(), imported.facts);
            keep_json(self.store(), Item::ImportedScenarios, &kept)?;
            self.forget_facts();
        }
        Ok(json!({ "reviews": reviews, "marks": marks }))
    }
}

/// A recording exported, and opened again.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Layout};
    use crate::store::Part;

    /// A recording with a review and a run window exports its files and its manifest entry; opened as another
    /// recording (an upload), it gets the same review and run window back, and a path of another kind is refused.
    #[test]
    fn a_recording_goes_out_and_back() {
        let dir = std::env::temp_dir().join(format!("aimview-export-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let vods = dir.join("vods");
        std::fs::create_dir_all(vods.join("Air")).unwrap();
        std::fs::write(vods.join("Air").join("Air - 100 - 2026.10.01-12.00.00.mp4"), b"video").unwrap();
        std::fs::create_dir_all(dir.join("data").join("uploads")).unwrap();
        std::fs::write(dir.join("data").join("uploads").join("copy.mp4"), b"video").unwrap();
        let mut config = Config::new(dir.join("data"), Layout::App, dir.join("models"));
        config.vods = Some(vods);
        let library = Library::open(config).unwrap();
        let (id, model) = ("Air/Air - 100 - 2026.10.01-12.00.00.mp4", ReviewBy::Model("m1".into()));
        library.store().write(Item::ReviewPart(id, &model, Part::Tracks), b"{\"frames\":[]}").unwrap();
        library.store().write(Item::Mark(id, Mark::RunWindow), b"{\"start\":5}").unwrap();

        let body =
            library.export(br#"{"ids": ["Air/Air - 100 - 2026.10.01-12.00.00.mp4"], "mode": "server"}"#).unwrap();
        let files = crate::batch::read(&body).unwrap();
        let paths: Vec<&str> = files.iter().map(|file| file.path).collect();
        let folder = "recordings/01 Air_-_100_-_2026.10.01-12.00.00";
        assert_eq!(paths, [&format!("{folder}/reviews/m1/tracks.json"), &format!("{folder}/marks/run.json"), MANIFEST]);
        let manifest: Value = serde_json::from_slice(files[2].bytes).unwrap();
        assert_eq!(manifest["recordings"][0]["video"], "Air - 100 - 2026.10.01-12.00.00.mp4");
        assert_eq!(manifest["recordings"][0]["reviews"], json!(["m1"]));

        let mut back = Vec::new();
        for file in &files[..2] {
            crate::batch::push(&mut back, file.path.strip_prefix(&format!("{folder}/")).unwrap(), 0.0, file.bytes);
        }
        let facts = br#"{"scenario": "Air", "facts": {"kind": "tracking", "limit": 60.0, "targets": 1, "reload": null, "hitbox": null}}"#;
        crate::batch::push(&mut back, FACTS_FILE, 0.0, facts);
        let kept = library.import_recording("uploads/copy.mp4", &back).unwrap();
        assert_eq!(kept, json!({ "reviews": 1, "marks": 1 }));
        let copy = "uploads/copy.mp4";
        assert_eq!(
            library.store().read(Item::ReviewPart(copy, &model, Part::Tracks)).unwrap().unwrap(),
            b"{\"frames\":[]}"
        );
        assert_eq!(library.facts().get("air").map(|facts| facts.limit), Some(Some(60.0)));
        let mut wrong = Vec::new();
        crate::batch::push(&mut wrong, "reviews/m1/../../settings.json", 0.0, b"{}");
        assert_eq!(library.import_recording(copy, &wrong).unwrap_err().status, 400);
        drop(library);
        let _ = std::fs::remove_dir_all(dir);
    }
}
