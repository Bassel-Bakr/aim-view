//! What the library keeps and how much space each part takes, for the data panel (docs/storage-design.md, "Space and
//! cleanup"), and removing a part the user can do without. In: the store's sizes (store.rs), the data folder's folders
//! and the settings. Out: GET /api/storage, {total, parts: [{id, kind, bytes, ...}]}, and POST /api/storage?remove=,
//! after which the database gives back the space freed.
//!
//! Parts the user made (marks and areas, cut-off labels, mouse logs) are listed but never removed here: the panel
//! names where to download them. Removable: a model's reviews, the videos added, KovaaK's files the browser keeps, the
//! files the data folder held before its database imported them, and the ffmpeg the desktop app downloaded.

use std::path::PathBuf;

use serde_json::{Value, json};

use super::{Answer, Failure, Library};
use crate::config::Ffmpeg;
use crate::store::{Item, StoreUsage, folder_size};

/// A part's id for one model's reviews: this, then the model's name ("" the old reviews).
const REVIEW_PREFIX: &str = "review:";
/// The data folder's own files the database imports (store.rs `Files`' names), for the old files' part.
const IMPORTED_FILES: [Item<'static>; 8] = [
    Item::Settings,
    Item::AreaKinds,
    Item::AreaExamples,
    Item::UploadAreas,
    Item::Ids(crate::store::IdList::NotAimTrainer),
    Item::Ids(crate::store::IdList::LabelSkipped),
    Item::Ids(crate::store::IdList::FaintSkipped),
    Item::CutoffRows,
];

/// One part of what is kept.
fn part(id: &str, kind: &str, bytes: u64, removable: bool) -> Value {
    json!({ "id": id, "kind": kind, "bytes": bytes, "removable": removable })
}

impl Library {
    /// The store as the data panel needs it: the space its parts take, and their removal.
    fn usage(&self) -> &dyn StoreUsage {
        self.store()
    }

    /// The models models.json lists.
    fn listed_models(&self) -> Vec<String> {
        let info = self.models_info().unwrap_or(Value::Null);
        info["models"].as_object().map(|models| models.keys().cloned().collect()).unwrap_or_default()
    }

    /// The files and folders the data folder held before its database imported them (none without a database).
    fn imported_paths(&self) -> Vec<PathBuf> {
        if !self.config.database {
            return Vec::new();
        }
        let files = IMPORTED_FILES.iter().map(|item| self.folders.files.join(item.file_name()));
        let folders = [self.folders.recordings.clone(), self.folders.cutoff.clone()];
        files.chain(folders).filter(|path| crate::disk::exists(path)).collect()
    }

    /// The folder ffmpeg was downloaded into, when the app downloads it.
    fn ffmpeg_folder(&self) -> Option<PathBuf> {
        match &self.config.ffmpeg {
            Ffmpeg::Download(folder) => Some(folder.clone()),
            _ => None,
        }
    }

    /// The size of a file or a folder (and below); 0 when it is not there.
    fn path_size(path: &std::path::Path) -> u64 {
        match crate::disk::metadata(path) {
            Ok(metadata) if metadata.is_dir() => folder_size(path).0,
            Ok(metadata) => metadata.len(),
            Err(_) => 0,
        }
    }

    /// GET /api/storage: every part of what the library keeps, with its bytes, and the total. A model's reviews say
    /// how many recordings they cover and whether models.json still lists the model; KovaaK's files say how many of
    /// each kind are kept; folders say how many files they hold.
    pub fn storage(&self) -> Answer<Value> {
        let usage = self.usage();
        let listed = self.listed_models();
        let mut parts = Vec::new();
        for size in usage.review_sizes() {
            let mut entry = part(&format!("{REVIEW_PREFIX}{}", size.model), "reviews", size.bytes, true);
            entry["model"] = json!(size.model);
            entry["recordings"] = json!(size.recordings);
            entry["listed"] = json!(listed.contains(&size.model));
            parts.push(entry);
        }
        parts.push(part("marks", "marks", usage.marks_size(), false));
        parts.push(part("cutoff", "cutoff", usage.cutoff_size(), false));
        if let Some(kovaak) = self.store().kovaak() {
            let (stats, scenarios, bytes) = kovaak.kovaak_size()?;
            let mut entry = part("kovaak", "kovaak", bytes, true);
            entry["stats"] = json!(stats);
            entry["scenarios"] = json!(scenarios);
            parts.push(entry);
        }
        for (id, dir, removable) in [("uploads", self.uploads(), true), ("mouse", self.folders.mouse.clone(), false)] {
            let (bytes, files) = folder_size(&dir);
            let mut entry = part(id, id, bytes, removable);
            entry["files"] = json!(files);
            parts.push(entry);
        }
        let imported = self.imported_paths();
        if !imported.is_empty() {
            parts.push(part("old_files", "old_files", imported.iter().map(|path| Self::path_size(path)).sum(), true));
        }
        if let Some(folder) = self.ffmpeg_folder().filter(|folder| crate::disk::is_dir(folder)) {
            parts.push(part("ffmpeg", "ffmpeg", folder_size(&folder).0, true));
        }
        let database = usage.file_size();
        let outside: u64 = parts
            .iter()
            .filter(|entry| ["uploads", "mouse", "old_files", "ffmpeg"].contains(&entry["kind"].as_str().unwrap_or("")))
            .filter_map(|entry| entry["bytes"].as_u64())
            .sum();
        let kept: u64 = parts.iter().filter_map(|entry| entry["bytes"].as_u64()).sum();
        let total = database.map_or(kept, |file| file + outside);
        Ok(json!({ "total": total, "database": database, "parts": parts }))
    }

    /// POST /api/storage?remove=<id>: forgets one removable part (a model's reviews, the videos added, KovaaK's files,
    /// the imported old files, the downloaded ffmpeg), then compacts the database. Another id is refused (400).
    /// Answers the parts as GET /api/storage does now.
    pub fn remove_storage(&self, id: &str) -> Answer<Value> {
        let failed = |error: std::io::Error| Failure::from(format!("{id}: {error}"));
        if let Some(model) = id.strip_prefix(REVIEW_PREFIX) {
            self.usage().remove_reviews(model).map_err(failed)?;
        } else {
            match id {
                "uploads" => {
                    let dir = self.uploads();
                    if crate::disk::is_dir(&dir) {
                        crate::disk::remove_dir_all(&dir).map_err(failed)?;
                    }
                }
                "kovaak" => {
                    let kovaak =
                        self.store().kovaak().ok_or_else(|| Failure::bad("KovaaK's files are not kept here"))?;
                    kovaak.clear_kovaak().map_err(failed)?;
                    #[cfg(not(feature = "native"))]
                    self.kovaak_changed()?;
                }
                "old_files" => {
                    for path in self.imported_paths() {
                        let removed = if crate::disk::is_dir(&path) {
                            crate::disk::remove_dir_all(&path)
                        } else {
                            crate::disk::remove_file(&path)
                        };
                        removed.map_err(failed)?;
                    }
                }
                "ffmpeg" => {
                    let folder = self.ffmpeg_folder().ok_or_else(|| Failure::bad("ffmpeg is not downloaded here"))?;
                    crate::disk::remove_dir_all(&folder).map_err(failed)?;
                }
                _ => return Err(Failure::bad(format!("{id} cannot be removed here"))),
            }
        }
        self.usage().compact().map_err(failed)?;
        self.storage()
    }
}

/// The data panel's parts, listed and removed.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Layout};
    use crate::store::{Part, ReviewBy};

    /// The part with this id in an answer.
    fn find<'a>(answer: &'a Value, id: &str) -> Option<&'a Value> {
        answer["parts"].as_array()?.iter().find(|entry| entry["id"] == id)
    }

    /// Two models' reviews and an upload are listed with their sizes; removing one model's reviews and the uploads
    /// leaves the rest, and a part the user made cannot be removed.
    #[test]
    fn lists_and_removes_parts() {
        for database in [true, false] {
            lists_and_removes(database);
        }
    }

    /// `lists_and_removes_parts` with the database or with files.
    fn lists_and_removes(database: bool) {
        let dir = std::env::temp_dir().join(format!("aimview-usage-{database}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut config = Config::new(dir.clone(), Layout::App, dir.join("models"));
        config.database = database;
        let library = Library::open(config).unwrap();
        let (a, b) = (ReviewBy::Model("a".into()), ReviewBy::Model("b".into()));
        for id in ["x/x - 1 - 2026.10.01-12.00.00.mp4", "y/y - 1 - 2026.10.01-12.00.00.mp4"] {
            library.store().write(Item::ReviewPart(id, &a, Part::Tracks), &[b'1'; 5000]).unwrap();
        }
        library.store().write(Item::ReviewPart("x/x - 1 - 2026.10.01-12.00.00.mp4", &b, Part::Tracks), b"{}").unwrap();
        std::fs::create_dir_all(dir.join("uploads")).unwrap();
        std::fs::write(dir.join("uploads").join("v.mp4"), [0u8; 300]).unwrap();
        let answer = library.storage().unwrap();
        let reviews_a = find(&answer, "review:a").unwrap();
        assert_eq!((reviews_a["recordings"].as_u64(), reviews_a["listed"].as_bool()), (Some(2), Some(false)));
        assert!(reviews_a["bytes"].as_u64().unwrap() > 0);
        assert_eq!(find(&answer, "uploads").unwrap()["bytes"], 300);
        assert_eq!(answer["database"].as_u64().is_some(), database);
        let after = library.remove_storage("review:a").unwrap();
        assert!(find(&after, "review:a").is_none() && find(&after, "review:b").is_some());
        let after = library.remove_storage("uploads").unwrap();
        assert_eq!(find(&after, "uploads").unwrap()["bytes"], 0);
        assert_eq!(library.remove_storage("marks").unwrap_err().status, 400);
        drop(library);
        let _ = std::fs::remove_dir_all(dir);
    }
}
