//! The recordings: the list (python/server.py: Library.list), a recording's video from its id and its folder, videos
//! and stats files added from the user's computer, and each scenario's facts from its scenario file.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use aimview::scenario::Facts;
use serde_json::{Value, json};

use super::names::{free_name, local_stamp, parse_name, parse_video, slug};
use super::{Answer, Failure, Library, modified};

pub(crate) const VIDEO_TYPES: [&str; 4] = ["mp4", "mkv", "mov", "webm"];

/// Removes the upload bodies (`spool`'s ".incoming-<pid>-<n>.part" files) that a process other than `pid` left in
/// `dir`: a server that stopped mid-upload never moved them into place. Every other file stays.
pub(super) fn remove_stale_spools(dir: &Path, pid: u32) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let owner = name
            .strip_prefix(".incoming-")
            .and_then(|r| r.strip_suffix(".part"))
            .and_then(|r| r.split_once('-'))
            .filter(|(_, n)| n.parse::<u64>().is_ok())
            .and_then(|(p, _)| p.parse::<u32>().ok());
        if owner.is_some_and(|p| p != pid)
            && e.path().is_file()
            && let Err(err) = std::fs::remove_file(e.path())
        {
            eprintln!("{}: {err}", e.path().display());
        }
    }
}

impl Library {
    /// Where videos (and in stats/, stats files) added from the user's computer are kept.
    pub(crate) fn uploads(&self) -> PathBuf {
        self.folders.uploads.clone()
    }

    /// A recording's video, from its id: a path in the VODs folder, or "uploads/<name>".
    pub fn resolve(&self, id: &str) -> Answer<PathBuf> {
        let (root, rel) = match id.strip_prefix("uploads/") {
            Some(name) => (self.uploads(), name.to_string()),
            None => (self.vods().ok_or_else(|| Failure::missing("no VODs folder is chosen"))?, id.to_string()),
        };
        let p = root.join(&rel);
        let ok_type = p.extension().is_some_and(|e| VIDEO_TYPES.contains(&e.to_string_lossy().to_lowercase().as_str()));
        let inside = p.canonicalize().ok().zip(root.canonicalize().ok()).is_some_and(|(p, r)| p.starts_with(r));
        if !ok_type || !inside || !p.is_file() {
            return Err(Failure::missing(format!("no recording {id}")));
        }
        Ok(p)
    }

    /// A recording's folder in the data folder (python/server.py: cache_dir): its reviews, areas, marks.
    pub fn review_dir(&self, id: &str) -> PathBuf {
        self.folders.recordings.join(slug(id))
    }

    /// Each scenario's facts by lower-case name, from the scenario folders' files, read once.
    pub(crate) fn facts(&self) -> Arc<HashMap<String, Facts>> {
        let mut cached = self.facts.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(f) = cached.as_ref() {
            return f.clone();
        }
        let mut files: Vec<PathBuf> = Vec::new();
        let sce = |dir: &Path, files: &mut Vec<PathBuf>| {
            for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let p = e.path();
                if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("sce")) {
                    files.push(p);
                }
            }
        };
        for folder in &self.config.scenarios {
            sce(folder, &mut files);
            for e in std::fs::read_dir(folder).into_iter().flatten().flatten() {
                if e.path().is_dir() {
                    sce(&e.path(), &mut files);
                }
            }
        }
        let mut out = HashMap::new();
        for p in files {
            let Ok(bytes) = std::fs::read(&p) else { continue };
            let name = p.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
            out.insert(name, aimview::scenario::facts(&String::from_utf8_lossy(&bytes)));
        }
        let out = Arc::new(out);
        *cached = Some(out.clone());
        out
    }

    /// The facts of a video's scenario (from its name), if its scenario file was found.
    pub(crate) fn facts_of(&self, video: &Path) -> Option<Facts> {
        let scenario = parse_video(video)?.0.to_lowercase();
        self.facts().get(&scenario).cloned()
    }

    fn kind(&self, scenario: &str) -> Value {
        self.facts().get(&scenario.to_lowercase()).map_or(Value::Null, |f| json!(f.kind))
    }

    /// The recordings, newest first (python/server.py: Library.list).
    pub fn recordings(&self) -> Answer<Value> {
        let (mut out, not_aim): (Vec<Value>, _) = (Vec::new(), self.not_aim());
        if let Some(vods) = self.vods() {
            for folder in std::fs::read_dir(&vods).into_iter().flatten().flatten() {
                if !folder.path().is_dir() {
                    continue;
                }
                for e in std::fs::read_dir(folder.path()).into_iter().flatten().flatten() {
                    let p = e.path();
                    let name = e.file_name().to_string_lossy().into_owned();
                    let Some((scenario, score, stamp)) = parse_name(&name) else { continue };
                    let id = format!("{}/{name}", folder.file_name().to_string_lossy());
                    out.push(json!({
                        "id": id, "scenario": scenario, "kind": self.kind(&scenario), "score": score, "stamp": stamp,
                        "mtime": modified(&p), "size": p.metadata().map_or(0, |m| m.len()),
                        "stats": self.stats_of(&id, &p).is_some(), "analysed": self.reviewed(&id), "not_aim": not_aim.contains(&id),
                    }));
                }
            }
        }
        for e in std::fs::read_dir(self.uploads()).into_iter().flatten().flatten() {
            let p = e.path();
            let ext = p.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
            if !p.is_file() || !VIDEO_TYPES.contains(&ext.as_str()) {
                continue;
            }
            let name = e.file_name().to_string_lossy().into_owned();
            let id = format!("uploads/{name}");
            let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let (scenario, score, stamp) = match parse_video(&p) {
                Some((s, score, stamp)) => (s, Some(score), stamp),
                None => (stem, None, local_stamp(modified(&p))),
            };
            out.push(json!({
                "id": id, "scenario": scenario, "kind": self.kind(&scenario), "score": score, "stamp": stamp,
                "mtime": modified(&p), "size": p.metadata().map_or(0, |m| m.len()),
                "stats": self.stats_of(&id, &p).is_some(), "uploaded": true, "analysed": self.reviewed(&id), "not_aim": not_aim.contains(&id),
            }));
        }
        out.sort_by(|a, b| b["mtime"].as_f64().unwrap_or(0.0).total_cmp(&a["mtime"].as_f64().unwrap_or(0.0)));
        Ok(Value::Array(out))
    }

    /// A video added from this computer (kept in the uploads), or a stats file for a recording (`id`), which it is then
    /// paired with. Nothing is overwritten.
    pub fn upload(&self, name: &str, id: Option<&str>, body: &[u8]) -> Answer<Value> {
        self.add_upload(name, id, |dest| std::fs::write(dest, body))
    }

    /// The same for an upload already on disk (`file`, from `spool`, written as it arrived): it is moved into place,
    /// never read into memory.
    pub fn upload_file(&self, name: &str, id: Option<&str>, file: &Path) -> Answer<Value> {
        self.add_upload(name, id, |dest| std::fs::rename(file, dest).or_else(|_| std::fs::copy(file, dest).and_then(|_| std::fs::remove_file(file))))
    }

    /// A new file in the uploads folder for an upload's body, written as it arrives (the HTTP server does so), which
    /// `upload_file` then moves into place. Its name is not a video's or a stats file's: the list never shows it.
    pub fn spool(&self) -> Answer<PathBuf> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        std::fs::create_dir_all(self.uploads()).map_err(|e| e.to_string())?;
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        Ok(self.uploads().join(format!(".incoming-{}-{n}.part", std::process::id())))
    }

    /// An upload's checks and its place in the uploads; `save` writes it there.
    fn add_upload(&self, name: &str, id: Option<&str>, save: impl FnOnce(&Path) -> std::io::Result<()>) -> Answer<Value> {
        let name = Path::new(name).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let ext = Path::new(&name).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        let (dest, id) = if ext == "csv" {
            let id = id.ok_or_else(|| Failure::bad("a stats file needs id=<the recording's id>"))?;
            self.resolve(id)?;
            (free_name(self.uploads().join("stats").join(&name)), Some(id))
        } else if VIDEO_TYPES.contains(&ext.as_str()) {
            (free_name(self.uploads().join(&name)), None)
        } else {
            return Err(Failure::bad(format!("not a video or a stats .csv: {name}")));
        };
        std::fs::create_dir_all(dest.parent().unwrap_or(&self.uploads())).map_err(|e| e.to_string())?;
        save(&dest).map_err(|e| e.to_string())?;
        let saved = dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match id {
            None => Ok(json!({ "id": format!("uploads/{saved}"), "saved": saved })),
            Some(id) => {
                let change = self.set_stats(id, &json!({ "file": saved, "source": "upload" }))?;
                Ok(json!({ "id": id, "saved": saved, "job": change["job"], "stats": change["stats"] }))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::config::{Config, Layout};
    use crate::library::Library;

    /// Opening the library removes the upload bodies another process left behind, and keeps this process's and every
    /// other file.
    #[test]
    fn opening_removes_other_processes_upload_bodies() {
        let dir = std::env::temp_dir().join(format!("aimview-spools-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let config = Config::new(dir.clone(), Layout::App, dir.join("models"));
        let uploads = config.folders().uploads;
        std::fs::create_dir_all(&uploads).unwrap();
        let other = std::process::id().wrapping_add(1);
        let mine = format!(".incoming-{}-0.part", std::process::id());
        let kept = [mine.as_str(), "run.mp4", ".incoming-x-0.part", ".incoming-12.part", "incoming-12-0.part"];
        for name in kept.iter().copied().chain([format!(".incoming-{other}-3.part").as_str()]) {
            std::fs::write(uploads.join(name), b"x").unwrap();
        }
        let lib = Library::open(config).unwrap();
        let mut left: Vec<String> = std::fs::read_dir(lib.uploads())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        let mut want: Vec<String> = kept.iter().map(|s| s.to_string()).collect();
        want.sort();
        assert_eq!(left, want);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
