//! The desktop app's library: the user's recordings (the VODs folder), KovaaK's stats files and scenarios, the models,
//! and each recording's reviews, kept in the app's data folder. It answers what the review server (python/server.py)
//! answers, so the app's window uses the server mode's services; the review itself runs natively (review.rs).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use aimview::scenario::Facts;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::review::{Request, TimeWindow, review};
use crate::run_window::{RunMarks, covers};

const VIDEO_TYPES: [&str; 4] = ["mp4", "mkv", "mov", "webm"];
/// Stats files offered to pair with a recording.
const CANDIDATES: usize = 40;
/// Seconds before the stats folder is listed again (runs played while the app is open are found).
const INDEX_AGE: u64 = 60;
/// The model new reviews use until the user picks one (infer.BEST).
const BEST: &str = "full_v3";
/// KovaaK's folder where Steam puts it.
const KOVAAK_DEFAULT: &str = r"C:\Program Files (x86)\Steam\steamapps\common\FPSAimTrainer\FPSAimTrainer";

/// An error for the page: its message, and the HTTP status the API answers with.
pub struct Failure {
    pub status: u16,
    pub message: String,
}

impl Failure {
    pub fn missing(what: impl Into<String>) -> Failure {
        Failure { status: 404, message: what.into() }
    }
    pub fn bad(what: impl Into<String>) -> Failure {
        Failure { status: 400, message: what.into() }
    }
}

impl From<String> for Failure {
    fn from(message: String) -> Failure {
        Failure { status: 500, message }
    }
}

pub type Answer<T> = Result<T, Failure>;

/// What the user set, kept in the data folder: the VODs folder (OBS's recordings, one folder per scenario), KovaaK's
/// folder (FPSAimTrainer, holding stats/ and the scenarios) and the model new reviews use.
#[derive(Clone, Serialize, Deserialize)]
struct Settings {
    vods: Option<PathBuf>,
    kovaak: PathBuf,
    model: String,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { vods: None, kovaak: PathBuf::from(KOVAAK_DEFAULT), model: BEST.into() }
    }
}

/// A review job: its stage, how far it is (frames), and at the end its time or its error.
#[derive(Clone, Serialize)]
pub struct Job {
    stage: String,
    done: usize,
    total: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    model: String,
}

impl Job {
    fn new(stage: &str, model: &str) -> Job {
        Job { stage: stage.into(), done: 0, total: 1, seconds: None, error: None, model: model.into() }
    }
}

/// A stats file in KovaaK's folder: when its run ended (seconds, see `stamp_seconds`), its name and its time stamp.
struct StatsEntry {
    t: f64,
    name: String,
    stamp: String,
}

/// KovaaK's stats files by scenario name, and when the folder was listed.
#[derive(Default)]
struct StatsIndex {
    by_scenario: HashMap<String, Vec<StatsEntry>>,
    listed: Option<Instant>,
}

/// The user's choice of stats file for a recording (stats.json): a file and where it is, or no file.
#[derive(Serialize, Deserialize)]
struct Pick {
    file: Option<String>,
    source: String,
}

pub struct Library {
    data: PathBuf,
    models: PathBuf,
    settings: Mutex<Settings>,
    stats: Mutex<StatsIndex>,
    facts: Mutex<Option<Arc<HashMap<String, Facts>>>>,
    jobs: Mutex<HashMap<String, Arc<Mutex<Job>>>>,
}

/// A recording's name as KovOBS writes it: "<scenario> - <score> - <yyyy.mm.dd-hh.mm.ss>.mp4".
fn parse_name(name: &str) -> Option<(String, f64, String)> {
    let rest = name.strip_suffix(".mp4")?;
    let (rest, stamp) = rest.rsplit_once(" - ")?;
    let (scenario, score) = rest.rsplit_once(" - ")?;
    let numeric = !score.is_empty() && score.chars().all(|c| c.is_ascii_digit() || c == '-' || c == '.');
    if !numeric || scenario.is_empty() || stamp_seconds(stamp).is_none() {
        return None;
    }
    Some((scenario.to_string(), score.parse().ok()?, stamp.to_string()))
}

/// A stats file's name as KovaaK writes it: "<scenario> - Challenge - <stamp> Stats.csv".
fn parse_stats_name(name: &str) -> Option<(String, String)> {
    let rest = name.strip_suffix(" Stats.csv")?;
    let (rest, stamp) = rest.rsplit_once(" - ")?;
    let scenario = rest.strip_suffix(" - Challenge")?;
    stamp_seconds(stamp).map(|_| (scenario.to_string(), stamp.to_string()))
}

/// A file-name time stamp (yyyy.mm.dd-hh.mm.ss, local time) as seconds from 2000-01-01, for differences only. Some
/// KovOBS recordings from June 2026 are named with the year 0026: read as 2026.
fn stamp_seconds(stamp: &str) -> Option<f64> {
    let b = stamp.as_bytes();
    if b.len() != 19 || b[4] != b'.' || b[7] != b'.' || b[10] != b'-' || b[13] != b'.' || b[16] != b'.' {
        return None;
    }
    let num = |from: usize, to: usize| stamp.get(from..to)?.parse::<i64>().ok();
    let mut year = num(0, 4)?;
    if stamp.starts_with("00") {
        year += 2000;
    }
    let (month, day, h, m, s) = (num(5, 7)?, num(8, 10)?, num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || h > 23 || m > 59 || s > 60 {
        return None;
    }
    // days from the civil date (Howard Hinnant's algorithm), from 2000-01-01
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 730_425;
    Some((days * 86_400 + h * 3600 + m * 60 + s) as f64)
}

/// A recording's folder in the data folder: its file name's stem, with anything but word characters, dots and
/// dashes as "_" (python/server.py: cache_dir).
fn slug(id: &str) -> String {
    let stem = Path::new(id).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let mut out = String::new();
    let mut gap = false;
    for c in stem.chars() {
        if c.is_alphanumeric() || matches!(c, '_' | '.' | '-') {
            out.push(c);
            gap = false;
        } else if !gap {
            out.push('_');
            gap = true;
        }
    }
    out
}

/// p, or p with " (2)", " (3)" and so on in its name when that file is there already: nothing is overwritten.
fn free_name(p: PathBuf) -> PathBuf {
    let (stem, ext) = (
        p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
        p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default(),
    );
    let mut out = p.clone();
    let mut n = 2;
    while out.exists() {
        out = p.with_file_name(format!("{stem} ({n}){ext}"));
        n += 1;
    }
    out
}

fn modified(p: &Path) -> f64 {
    p.metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0.0, |d| d.as_secs_f64())
}

fn read_json<T: for<'a> Deserialize<'a>>(p: &Path) -> Option<T> {
    serde_json::from_slice(&std::fs::read(p).ok()?).ok()
}

fn write_json(p: &Path, v: &impl Serialize) -> Answer<()> {
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(p, serde_json::to_vec(v).map_err(|e| e.to_string())?).map_err(|e| e.to_string().into())
}

impl Library {
    /// The library in the app's data folder, with the models (models.json and the _u8in exports) in `models`.
    pub fn new(data: PathBuf, models: PathBuf) -> Library {
        let settings = read_json(&data.join("settings.json")).unwrap_or_default();
        Library {
            data,
            models,
            settings: Mutex::new(settings),
            stats: Mutex::default(),
            facts: Mutex::default(),
            jobs: Mutex::default(),
        }
    }

    fn settings(&self) -> Settings {
        self.settings.lock().map(|s| s.clone()).unwrap_or_default()
    }

    fn save_settings(&self, change: impl FnOnce(&mut Settings)) -> Answer<()> {
        let mut s = self.settings.lock().map_err(|_| "the settings are broken".to_string())?;
        change(&mut s);
        write_json(&self.data.join("settings.json"), &*s)
    }

    /// The VODs folder the user chose.
    pub fn set_vods(&self, folder: PathBuf) -> Answer<Value> {
        self.save_settings(|s| s.vods = Some(folder.clone()))?;
        Ok(json!({ "folder": folder }))
    }

    fn uploads(&self) -> PathBuf {
        self.data.join("uploads")
    }

    fn stats_folder(&self) -> PathBuf {
        self.settings().kovaak.join("stats")
    }

    /// Each scenario's facts by lower-case name, from KovaaK's scenario files and the workshop's, read once.
    fn facts(&self) -> Arc<HashMap<String, Facts>> {
        let mut cached = self.facts.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(f) = cached.as_ref() {
            return f.clone();
        }
        let kovaak = self.settings().kovaak;
        let mut files: Vec<PathBuf> = Vec::new();
        let sce = |dir: &Path, files: &mut Vec<PathBuf>| {
            for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let p = e.path();
                if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("sce")) {
                    files.push(p);
                }
            }
        };
        sce(&kovaak.join("Saved").join("SaveGames").join("Scenarios"), &mut files);
        // steamapps/common/FPSAimTrainer/FPSAimTrainer: the workshop is steamapps/workshop/content/824270
        if let Some(steamapps) = kovaak.ancestors().nth(3) {
            for e in std::fs::read_dir(steamapps.join("workshop").join("content").join("824270")).into_iter().flatten().flatten() {
                sce(&e.path(), &mut files);
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

    /// KovaaK's stats files, listed again once the listing is a minute old.
    fn with_stats<T>(&self, use_index: impl FnOnce(&HashMap<String, Vec<StatsEntry>>) -> T) -> T {
        let mut index = self.stats.lock().unwrap_or_else(|e| e.into_inner());
        if index.listed.is_none_or(|t| t.elapsed().as_secs() > INDEX_AGE) {
            let mut by_scenario: HashMap<String, Vec<StatsEntry>> = HashMap::new();
            for e in std::fs::read_dir(self.stats_folder()).into_iter().flatten().flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if let Some((scenario, stamp)) = parse_stats_name(&name) {
                    let t = stamp_seconds(&stamp).unwrap_or(0.0);
                    by_scenario.entry(scenario).or_default().push(StatsEntry { t, name, stamp });
                }
            }
            *index = StatsIndex { by_scenario, listed: Some(Instant::now()) };
        }
        use_index(&index.by_scenario)
    }

    fn stats_for(&self, scenario: &str, stamp: &str) -> Option<PathBuf> {
        let t = stamp_seconds(stamp)?;
        let name = self.with_stats(|index| {
            index
                .get(scenario)?
                .iter()
                .filter(|e| (e.t - t).abs() <= 5.0)
                .min_by(|a, b| (a.t - t).abs().total_cmp(&(b.t - t).abs()).then(a.name.cmp(&b.name)))
                .map(|e| e.name.clone())
        })?;
        Some(self.stats_folder().join(name))
    }

    /// A recording's video, from its id: a path in the VODs folder, or "uploads/<name>".
    pub fn resolve(&self, id: &str) -> Answer<PathBuf> {
        let (root, rel) = match id.strip_prefix("uploads/") {
            Some(name) => (self.uploads(), name.to_string()),
            None => (self.settings().vods.ok_or_else(|| Failure::missing("no VODs folder is chosen"))?, id.to_string()),
        };
        let p = root.join(&rel);
        let ok_type = p.extension().is_some_and(|e| VIDEO_TYPES.contains(&e.to_string_lossy().to_lowercase().as_str()));
        let inside = p.canonicalize().ok().zip(root.canonicalize().ok()).is_some_and(|(p, r)| p.starts_with(r));
        if !ok_type || !inside || !p.is_file() {
            return Err(Failure::missing(format!("no recording {id}")));
        }
        Ok(p)
    }

    fn review_dir(&self, id: &str) -> PathBuf {
        self.data.join("reviews").join(slug(id))
    }

    fn pairing(&self, id: &str) -> Option<Pick> {
        read_json(&self.review_dir(id).join("stats.json"))
    }

    fn stats_file(&self, name: &str, source: &str) -> Answer<PathBuf> {
        let plain = Path::new(name).file_name().is_some_and(|f| f == name) && name.to_lowercase().ends_with(".csv");
        if !plain {
            return Err(Failure::bad(format!("not a stats file: {name}")));
        }
        match source {
            "kovaak" => Ok(self.stats_folder().join(name)),
            "upload" => Ok(self.uploads().join("stats").join(name)),
            _ => Err(Failure::bad(format!("no stats files kept in {source}"))),
        }
    }

    /// The recording's stats file: the user's choice (None when it is gone), else one uploaded beside it (same name,
    /// .csv), else by name and time.
    fn stats_of(&self, id: &str, video: &Path) -> Option<PathBuf> {
        if let Some(pick) = self.pairing(id) {
            return pick.file.and_then(|f| self.stats_file(&f, &pick.source).ok()).filter(|p| p.is_file());
        }
        let beside = video.with_extension("csv");
        if id.starts_with("uploads/") && beside.is_file() {
            return Some(beside);
        }
        let name = video.with_extension("mp4");
        let (scenario, _, stamp) = parse_name(&name.file_name()?.to_string_lossy())?;
        self.stats_for(&scenario, &stamp)
    }

    /// The review to show: (model, folder): the chosen model's, else the newest by another model; with none, the
    /// chosen model's folder for a new one.
    fn shown(&self, id: &str) -> (String, PathBuf) {
        let model = self.settings().model;
        let models = self.review_dir(id).join("models");
        let own = models.join(&model);
        if own.join("tracks.json").is_file() {
            return (model, own);
        }
        let other = std::fs::read_dir(&models)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.join("tracks.json").is_file())
            .max_by(|a, b| modified(&a.join("tracks.json")).total_cmp(&modified(&b.join("tracks.json"))));
        match other {
            Some(dir) => (dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), dir),
            None => (model, own),
        }
    }

    fn reviewed(&self, id: &str) -> bool {
        std::fs::read_dir(self.review_dir(id).join("models"))
            .into_iter()
            .flatten()
            .flatten()
            .any(|e| e.path().join("tracks.json").is_file())
    }

    fn kind(&self, scenario: &str) -> Value {
        self.facts().get(&scenario.to_lowercase()).map_or(Value::Null, |f| json!(f.kind))
    }

    /// The recordings, newest first (python/server.py: Library.list).
    pub fn recordings(&self) -> Answer<Value> {
        let mut out: Vec<Value> = Vec::new();
        if let Some(vods) = self.settings().vods {
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
                        "stats": self.stats_of(&id, &p).is_some(), "analysed": self.reviewed(&id), "not_aim": false,
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
            let parsed = parse_name(&p.with_extension("mp4").file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
            let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let (scenario, score, stamp) = match parsed {
                Some((s, score, stamp)) => (s, Some(score), stamp),
                None => (stem, None, local_stamp(modified(&p))),
            };
            out.push(json!({
                "id": id, "scenario": scenario, "kind": self.kind(&scenario), "score": score, "stamp": stamp,
                "mtime": modified(&p), "size": p.metadata().map_or(0, |m| m.len()),
                "stats": self.stats_of(&id, &p).is_some(), "uploaded": true, "analysed": self.reviewed(&id), "not_aim": false,
            }));
        }
        out.sort_by(|a, b| b["mtime"].as_f64().unwrap_or(0.0).total_cmp(&a["mtime"].as_f64().unwrap_or(0.0)));
        Ok(Value::Array(out))
    }

    /// The stats file a recording uses and how it came to it, and the stats files to pair it with: its scenario's, or
    /// with q those of every scenario whose name holds q, nearest the recording's time first.
    pub fn stats_info(&self, id: &str, q: Option<&str>) -> Answer<Value> {
        let video = self.resolve(id)?;
        let (pick, file) = (self.pairing(id), self.stats_of(id, &video));
        let how = match &pick {
            Some(p) if p.file.is_none() => "none",
            Some(_) if file.is_none() => "gone",
            Some(p) if p.source == "upload" => "upload",
            Some(_) => "picked",
            None if file.is_none() => "missing",
            None if id.starts_with("uploads/") && file.as_ref().is_some_and(|f| f.parent() == video.parent()) => "beside",
            None => "found",
        };
        let parsed = parse_name(&video.with_extension("mp4").file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
        let scenario = parsed.as_ref().map_or_else(
            || video.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
            |(s, _, _)| s.clone(),
        );
        let t = parsed.and_then(|(_, _, stamp)| stamp_seconds(&stamp)).unwrap_or_else(|| {
            stamp_seconds(&local_stamp(modified(&video))).unwrap_or(0.0)
        });
        let text = q.unwrap_or(&scenario).trim().to_lowercase();
        let candidates = self.with_stats(|index| {
            let mut near: Vec<(f64, &str, &StatsEntry)> = index
                .iter()
                .filter(|(s, _)| if q.is_none() { s.to_lowercase() == text } else { s.to_lowercase().contains(&text) })
                .flat_map(|(s, entries)| entries.iter().map(move |e| ((e.t - t).abs(), s.as_str(), e)))
                .collect();
            near.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.2.t.total_cmp(&b.2.t)).then(a.1.cmp(b.1)));
            near.into_iter()
                .take(CANDIDATES)
                .map(|(_, s, e)| json!({ "name": e.name, "scenario": s, "stamp": e.stamp, "off": ((e.t - t) * 10.0).round() / 10.0 }))
                .collect::<Vec<_>>()
        });
        let file = file
            .and_then(|f| f.file_name().map(|n| n.to_string_lossy().into_owned()))
            .or_else(|| pick.and_then(|p| p.file));
        Ok(json!({ "file": file, "how": how, "scenario": scenario, "candidates": candidates }))
    }

    /// The user's choice of stats file ({file, source}, file null for none) or {auto: true} (by name and time again).
    /// The report is worked out when it is shown, so nothing is measured again here.
    pub fn set_stats(&self, id: &str, body: &Value) -> Answer<Value> {
        let video = self.resolve(id)?;
        let path = self.review_dir(id).join("stats.json");
        if body["auto"].as_bool() == Some(true) {
            let _ = std::fs::remove_file(&path);
        } else {
            let file = body["file"].as_str().map(str::to_string);
            let source = body["source"].as_str().unwrap_or("kovaak").to_string();
            if let Some(f) = &file
                && !self.stats_file(f, &source)?.is_file()
            {
                return Err(Failure::missing(f.clone()));
            }
            write_json(&path, &Pick { file, source })?;
        }
        let job = if self.reviewed(id) { Job::new("done", &self.shown(id).0) } else { Job::new("none", "") };
        Ok(json!({ "job": job, "stats": self.stats_of(id, &video).is_some() }))
    }

    /// A video added from this computer (copied into the app's uploads), or a stats file for a recording (`id`),
    /// which it is then paired with. Nothing is overwritten.
    pub fn upload(&self, name: &str, id: Option<&str>, body: &[u8]) -> Answer<Value> {
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
        std::fs::write(&dest, body).map_err(|e| e.to_string())?;
        let saved = dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match id {
            None => Ok(json!({ "id": format!("uploads/{saved}"), "saved": saved })),
            Some(id) => {
                let change = self.set_stats(id, &json!({ "file": saved, "source": "upload" }))?;
                Ok(json!({ "id": id, "saved": saved, "job": change["job"], "stats": change["stats"] }))
            }
        }
    }

    /// The models to pick from: the ones models.json describes that ship with the app (python/server.py: models).
    pub fn models(&self) -> Answer<Value> {
        let info: Value = read_json(&self.models.join("models.json")).ok_or("models.json is missing".to_string())?;
        let mut models = Vec::new();
        for (name, m) in info["models"].as_object().into_iter().flatten() {
            if !self.model_file(name).is_file() {
                continue;
            }
            let mut m = m.clone();
            m["name"] = json!(name);
            if m.get("label").is_none() {
                m["label"] = json!(name);
            }
            m["default"] = json!(name == BEST);
            m["available"] = json!(true);
            models.push(m);
        }
        Ok(json!({
            "chosen": self.settings().model, "device": "directml", "speed": info["speed"], "checks": info["checks"],
            "checked_on": info["checked_on"], "models": models,
        }))
    }

    fn model_file(&self, name: &str) -> PathBuf {
        self.models.join(format!("detector_{name}_u8in.onnx"))
    }

    /// The model new reviews use, kept for the next start.
    pub fn pick(&self, name: &str) -> Answer<Value> {
        if !self.model_file(name).is_file() {
            return Err(Failure::bad(format!("no model called {name} ships with the app")));
        }
        self.save_settings(|s| s.model = name.into())?;
        self.models()
    }

    pub fn job(&self, id: &str) -> Value {
        let jobs = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
        jobs.get(id).and_then(|j| j.lock().ok().map(|j| json!(*j))).unwrap_or_else(|| json!({ "stage": "none" }))
    }

    /// Reviews a recording: with again, a new review by the chosen model; else the one on show, or a new one when
    /// there is none. The review runs in a thread of its own; `job` follows it.
    pub fn analyse(self: &Arc<Self>, id: &str, again: bool) -> Answer<Value> {
        let mut jobs = self.jobs.lock().map_err(|_| "the jobs are broken".to_string())?;
        if let Some(job) = jobs.get(id)
            && let Ok(j) = job.lock()
            && j.stage != "done"
            && j.stage != "error"
        {
            return Ok(json!(*j));
        }
        let video = self.resolve(id)?;
        let (shown, dir) = self.shown(id);
        if !again && dir.join("tracks.json").is_file() {
            return Ok(json!(Job::new("done", &shown)));
        }
        let model = self.settings().model;
        let out = self.review_dir(id).join("models").join(&model);
        let scenario = parse_name(&video.with_extension("mp4").file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
            .map(|(s, _, _)| s.to_lowercase());
        let facts = scenario.and_then(|s| self.facts().get(&s).cloned());
        let cap = facts.as_ref().and_then(|f| f.targets).unwrap_or(0);
        let runs = if std::thread::available_parallelism().map_or(1, |n| n.get()) >= 8 { 2 } else { 1 };
        // the user's run window: only its part of the video is tracked
        let window = RunMarks::read(&self.review_dir(id)).tracked(facts.and_then(|f| f.limit));
        let req = Request { video, model: self.model_file(&model), batch: 4, cap, runs, window };
        let job = Arc::new(Mutex::new(Job::new("starting", &model)));
        jobs.insert(id.to_string(), job.clone());
        drop(jobs);
        let started = Instant::now();
        let first = json!(*job.lock().map_err(|_| "the job is broken".to_string())?);
        std::thread::spawn(move || {
            let progress = |stage: &str, done: usize, total: usize| {
                if let Ok(mut j) = job.lock() {
                    (j.stage, j.done, j.total) = (stage.into(), done, total);
                }
            };
            let outcome = crate::ffmpeg::ensure(|mb, of| progress("ffmpeg", mb, of)).and_then(|()| review(&req, &progress)).and_then(|r| {
                write_json(&out.join("tracks.json"), &r.tracks).map_err(|f| f.message)?;
                write_json(&out.join("readings.json"), &r.readings).map_err(|f| f.message)?;
                write_json(&out.join("hud.json"), &r.hud).map_err(|f| f.message)
            });
            if let Ok(mut j) = job.lock() {
                match outcome {
                    Ok(()) => {
                        (j.stage, j.done, j.total) = ("done".into(), 1, 1);
                        j.seconds = Some((started.elapsed().as_secs_f64() * 10.0).round() / 10.0);
                    }
                    Err(e) => (j.stage, j.error) = ("error".into(), Some(e)),
                }
            }
        });
        Ok(first)
    }

    /// The user's run window for the recording (all three null when none is marked).
    pub fn marks(&self, id: &str) -> Answer<Value> {
        self.resolve(id)?;
        Ok(json!(RunMarks::read(&self.review_dir(id))))
    }

    /// Keeps the run window ({start, end, length}; all null forgets it). The report reads it when it is shown; a review
    /// that tracked less of the video than the new window needs is made again.
    pub fn set_marks(self: &Arc<Self>, id: &str, body: &Value) -> Answer<Value> {
        let video = self.resolve(id)?;
        let marks = RunMarks::parse(body).map_err(Failure::bad)?;
        marks.save(&self.review_dir(id))?;
        let (shown, dir) = self.shown(id);
        let Some(tracks) = read_json::<Value>(&dir.join("tracks.json")) else { return Ok(json!(Job::new("none", ""))) };
        let tracked: Option<TimeWindow> = serde_json::from_value(tracks["window"].clone()).unwrap_or(None);
        let scenario = parse_name(&video.with_extension("mp4").file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
            .map(|(s, _, _)| s.to_lowercase());
        let limit = scenario.and_then(|s| self.facts().get(&s).and_then(|f| f.limit));
        if covers(tracked, marks.tracked(limit)) {
            return Ok(json!(Job::new("done", &shown)));
        }
        self.analyse(id, true)
    }

    /// The shown review's tracks (tracks.json), or None.
    pub fn tracks(&self, id: &str) -> Option<Vec<u8>> {
        std::fs::read(self.shown(id).1.join("tracks.json")).ok()
    }

    /// The shown review's report, worked out by the core (report.rs) from its tracks and the stats file, or without
    /// one from what the HUD read, else from the video alone; None without a review.
    pub fn report(&self, id: &str) -> Answer<Value> {
        let video = self.resolve(id)?;
        let (model, dir) = self.shown(id);
        let stats = self.stats_of(id, &video);
        let scenario = parse_name(&video.with_extension("mp4").file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
            .map(|(s, _, _)| s.to_lowercase());
        let facts = scenario.and_then(|s| self.facts().get(&s).cloned());
        let run = RunMarks::read(&self.review_dir(id));
        let Some(mut report) = crate::report::work_out(&dir, &video, stats.as_deref(), Some(run), facts.as_ref())? else {
            return Ok(Value::Null);
        };
        report["review_model"] = json!(model);
        Ok(report)
    }
}

/// A time (seconds since 1970) as a local file-name stamp, for a video added with a name of its own.
fn local_stamp(secs: f64) -> String {
    static OFFSET: OnceLock<i64> = OnceLock::new();
    let offset = *OFFSET.get_or_init(local_offset);
    let t = secs as i64 + offset;
    let (days, rem) = (t.div_euclid(86_400), t.rem_euclid(86_400));
    // the civil date from days since 1970 (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}.{month:02}.{day:02}-{:02}.{:02}.{:02}", rem / 3600, rem / 60 % 60, rem % 60)
}

/// This computer's offset from UTC in seconds (its time zone, now).
fn local_offset() -> i64 {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    #[cfg(windows)]
    {
        #[repr(C)]
        struct SystemTimeParts {
            year: u16,
            month: u16,
            weekday: u16,
            day: u16,
            hour: u16,
            minute: u16,
            second: u16,
            ms: u16,
        }
        unsafe extern "system" {
            fn GetLocalTime(t: *mut SystemTimeParts);
        }
        let mut t = SystemTimeParts { year: 0, month: 0, weekday: 0, day: 0, hour: 0, minute: 0, second: 0, ms: 0 };
        // SAFETY: GetLocalTime fills the struct it is given
        unsafe { GetLocalTime(&mut t) };
        let stamp = format!("{:04}.{:02}.{:02}-{:02}.{:02}.{:02}", t.year, t.month, t.day, t.hour, t.minute, t.second);
        if let Some(local) = stamp_seconds(&stamp) {
            // stamp_seconds counts from 2000-01-01; Unix time from 1970-01-01
            let local_unix = local as i64 + 946_684_800;
            return ((local_unix - now) as f64 / 900.0).round() as i64 * 900;
        }
    }
    let _ = now;
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_as_python_reads_them() {
        assert_eq!(
            parse_name("1wall 2targets xsmall - valorant - 558.46 - 2026.10.01-16.23.04.mp4"),
            Some(("1wall 2targets xsmall - valorant".into(), 558.46, "2026.10.01-16.23.04".into()))
        );
        assert_eq!(parse_name("clip.mp4"), None);
        assert_eq!(
            parse_stats_name("1wall 2targets xsmall - valorant - Challenge - 2026.10.01-16.23.04 Stats.csv"),
            Some(("1wall 2targets xsmall - valorant".into(), "2026.10.01-16.23.04".into()))
        );
        assert_eq!(stamp_seconds("2000.01.01-00.00.10"), Some(10.0));
        assert_eq!(stamp_seconds("2000.03.01-00.00.00"), Some(60.0 * 86_400.0));
        assert_eq!(stamp_seconds("0026.06.01-00.00.00"), stamp_seconds("2026.06.01-00.00.00"));
        assert_eq!(local_stamp(946_684_800.0 - local_offset() as f64), "2000.01.01-00.00.00");
        assert_eq!(slug("a/1wall - x (2).mp4"), "1wall_-_x_2_");
    }
}
