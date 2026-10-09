//! The browser build's own routes (api.rs): the page runs the review and the area finder itself and sends what they
//! give, which is kept as the native review keeps it; it adds raw mouse logs, chooses the VODs folder (a folder it
//! mounted), sends KovaaK's files the user chose, read once, and the detector labels a cut-off's submit made. In:
//! /api/job (POST), /api/reviewed, /api/found, /api/mouse_log, /api/folder, /api/kovaak, /api/kovaak_files and
//! /api/cutoff_labels. Out: the reviews, found areas, mouse logs, settings, KovaaK's runs and scenario facts and the
//! cut-off labels kept, and the jobs' state.

use std::path::{Path, PathBuf};

use aimview::areas::Found;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::names::free_name;
use super::reviews::{Job, ReviewFiles, keep_review, to_tenths};
use super::{Answer, Failure, Library};
use crate::pyjson;
use crate::review::Request;
use crate::store::{Item, ScenarioRow, StatsRow};

/// Where a stats file is in /kovaak (POST /api/kovaak_files).
const STATS_PREFIX: &str = "stats/";
/// Where scenario files are in /kovaak: the user's, and the workshop's (one folder per item).
const SCENARIO_PREFIXES: [&str; 2] = ["scenarios/", "workshop/"];
/// The rows of a submit's cut-off labels in its batch (POST /api/cutoff_labels).
const ROWS_FILE: &str = "rows.json";
/// Where a cut-off label's crop is, as its row names it.
const CROP_FOLDER: &str = "train/";

/// A cut-off label's row as the page sends it, written as the native submit writes the core's `CutoffRow` (the same
/// fields in the same order, numbers as floats): see aimview::faint::CutoffRow for each field.
#[derive(Deserialize, Serialize)]
struct PageCutoffRow {
    /// The crop's file, train/<stem>_<frame, 6 digits>.npz.
    file: String,
    /// The boxes the cut keeps, in crop pixels.
    boxes: Vec<[f64; 4]>,
    /// Always "correct".
    verdict: String,
    /// Every box the model gave in the crop.
    model: Vec<[f64; 4]>,
    /// Always "cutoff".
    source: String,
    /// The recording.
    video: String,
    /// The cut-off's offset.
    offset: f64,
    /// The score the cut was at.
    cut: f64,
}

/// Whether `path` names a crop as a row does: train/<name>.npz, one name with no folder above.
fn is_crop_path(path: &str) -> bool {
    path.strip_prefix(CROP_FOLDER)
        .is_some_and(|name| name.ends_with(".npz") && !name.contains(['/', '\\']) && !name.starts_with('.'))
}

/// A review the page ran (POST /api/reviewed): the files the native review writes, the area finder's find in the key
/// frames, its time (seconds) and device ("WebGPU", "WebAssembly"), and the model it ran (else the job's).
#[derive(Deserialize)]
struct PageReview {
    /// The tracks (tracks.json); they must have frames.
    tracks: Value,
    /// The camera's turn and the countdown (readings.json).
    readings: Value,
    /// What the HUD read (hud.json); null when none was read.
    #[serde(default)]
    hud: Value,
    /// What the area finder found in the key frames, when it read them.
    #[serde(default)]
    found: Option<Found>,
    /// The review's time in seconds.
    #[serde(default)]
    seconds: Option<f64>,
    /// Where the detector ran ("WebGPU", "WebAssembly").
    #[serde(default)]
    device: Option<String>,
    /// The model it ran; None: the job's.
    #[serde(default)]
    model: Option<String>,
}

/// How far a review the page runs is (POST /api/job), or its error.
#[derive(Deserialize)]
struct PageProgress {
    /// The review's stage; "done" and "error" are refused (the end comes by /api/reviewed or `error`).
    #[serde(default)]
    stage: Option<String>,
    /// Frames done.
    #[serde(default)]
    done: usize,
    /// Of how many.
    #[serde(default)]
    total: usize,
    /// Why the review failed; it ends the job.
    #[serde(default)]
    error: Option<String>,
}

/// What the page is to review, kept in its job (`Job::review`) until it reports progress: `review::Request`, the video
/// as its mounted path, the model by name, the device by its name ("webgpu" or "wasm"), the scenario's kind (null: not
/// known); not the runs (the page splits the recording itself).
pub(super) fn review_json(request: &Request, model: &str) -> Value {
    json!({
        "video": request.video, "model": model, "device": request.device.name(), "batch": request.batch,
        "cap": request.cap, "window": request.window, "areas": request.areas, "kind": request.kind,
    })
}

impl Library {
    /// POST /api/folder: the VODs folder, a folder the page mounted (/vods).
    pub fn choose_vods(&self, path: &str) -> Answer<Value> {
        if !path.starts_with('/') {
            return Err(Failure::bad(format!("not a folder the page mounted: {path}")));
        }
        self.set_vods(PathBuf::from(path))
    }

    /// POST /api/job: the progress of the review the page runs, so /api/job answers it as natively; an error ends it.
    /// A job that ended is left as it is.
    pub fn page_progress(&self, id: &str, body: &[u8]) -> Answer<Value> {
        let progress: PageProgress =
            serde_json::from_slice(body).map_err(|error| Failure::bad(format!("the progress: {error}")))?;
        let job = self.jobs.lock().map_err(|_| "the jobs are broken".to_string())?.get(id).cloned();
        let job = job.ok_or_else(|| Failure::missing(format!("no review of {id} is running")))?;
        let mut state = job.lock().map_err(|_| "the job is broken".to_string())?;
        if state.running() {
            state.review = None;
            match (progress.error, progress.stage) {
                (Some(error), _) => (state.stage, state.error) = ("error".into(), Some(error)),
                (None, Some(stage)) if stage == "done" || stage == "error" => {
                    return Err(Failure::bad("a review ends with /api/reviewed, or an error"));
                }
                (None, Some(stage)) => (state.stage, state.done, state.total) = (stage, progress.done, progress.total),
                (None, None) => return Err(Failure::bad("the progress needs a stage or an error")),
            }
        }
        Ok(json!(*state))
    }

    /// POST /api/reviewed: the review the page ran, its files written as the native review's end writes them (in
    /// models/<model>/ of the recording's folder) and its job done. With no job running (the data move) it only
    /// writes the files.
    pub fn review_done(&self, id: &str, body: &[u8]) -> Answer<Value> {
        let review: PageReview =
            serde_json::from_slice(body).map_err(|error| Failure::bad(format!("the review: {error}")))?;
        if !review.tracks["frames"].is_array() {
            return Err(Failure::bad("the review: its tracks have no frames"));
        }
        let job = self.jobs.lock().map_err(|_| "the jobs are broken".to_string())?.get(id).cloned();
        // a review the user cancelled keeps nothing, even when the page finished it before it heard
        if let Some(job) = &job
            && let Ok(state) = job.lock()
            && state.cancelled()
        {
            return Ok(json!(*state));
        }
        let job = job.filter(|job| job.lock().is_ok_and(|state| state.running()));
        let job_model = job.as_ref().and_then(|job| job.lock().ok().map(|state| state.model.clone()));
        let model = review.model.or(job_model).unwrap_or_else(|| self.model());
        if model.is_empty() || Path::new(&model).file_name().is_none_or(|file| file != model.as_str()) {
            return Err(Failure::bad(format!("not a model's name: {model}")));
        }
        let files = ReviewFiles { tracks: &review.tracks, readings: &review.readings, hud: &review.hud, kills: None };
        let outcome = keep_review(self.store(), id, &model, &files, review.found.as_ref());
        let Some(job) = job else {
            outcome?;
            return Ok(json!(Job::new("done", &model)));
        };
        let mut state = job.lock().map_err(|_| "the job is broken".to_string())?;
        state.review = None;
        match outcome {
            Ok(()) => {
                (state.stage, state.done, state.total) = ("done".into(), 1, 1);
                state.seconds = review.seconds.map(to_tenths);
                if review.device.is_some() {
                    state.device = review.device;
                }
                Ok(json!(*state))
            }
            Err(error) => {
                (state.stage, state.error) = ("error".into(), Some(error.clone()));
                Err(error.into())
            }
        }
    }

    /// POST /api/found: what the page's area finder found in the recording (the core's `Found`), kept as the native
    /// finder keeps it (finder.rs: `keep`); /api/find_areas then proposes from it.
    pub fn keep_found(&self, id: &str, body: &[u8]) -> Answer<Value> {
        let found: Found =
            serde_json::from_slice(body).map_err(|error| Failure::bad(format!("the found areas: {error}")))?;
        crate::finder::keep(self.store(), id, &found)?;
        Ok(json!({ "id": id, "kept": true }))
    }

    /// POST /api/mouse_log: a raw mouse log the user added (its bytes), kept in the mouse folder as the desktop's
    /// logger leaves them. The same log again is kept once; another of the same name gets a free name.
    pub fn keep_mouse_log(&self, name: &str, body: &[u8]) -> Answer<Value> {
        let plain = Path::new(name).file_name().is_some_and(|file| file == name) && name.ends_with(".bin");
        if !plain {
            return Err(Failure::bad(format!("not a mouse log (.bin): {name}")));
        }
        let dir = &self.folders.mouse;
        crate::disk::create_dir_all(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
        let mut path = dir.join(name);
        if crate::disk::read(&path).is_ok_and(|old| old == body) {
            return Ok(json!({ "saved": name }));
        }
        path = free_name(path);
        crate::disk::write(&path, body).map_err(|error| format!("{}: {error}", path.display()))?;
        Ok(json!({ "saved": path.file_name().map(|file| file.to_string_lossy().into_owned()) }))
    }

    /// GET /api/kovaak_files: what the browser keeps of KovaaK's files, for the page to send only the new or changed
    /// ones: {stats: [[name, size, time]], scenarios: [[path, size, time]]} (times in seconds since 1970). The stats
    /// files are those whose whole text is kept, so the page sends again one kept before without it.
    pub fn kovaak_files(&self) -> Answer<Value> {
        let kovaak =
            self.store().kovaak().ok_or_else(|| Failure::from("KovaaK's files are not kept here".to_string()))?;
        let stats = kovaak.stats_texts_kept()?;
        let scenarios = kovaak.scenarios()?;
        Ok(json!({
            "stats": stats.iter().map(|row| json!([row.name, row.size, row.modified])).collect::<Vec<_>>(),
            "scenarios": scenarios.iter().map(|row| json!([row.path, row.size, row.modified])).collect::<Vec<_>>(),
        }))
    }

    /// POST /api/kovaak_files: a batch of KovaaK's files the user chose, read once (batch.rs): each stats file's
    /// run and each scenario file's facts are kept, each kind in one transaction, and both are read again when next
    /// needed; each stats file's whole text is kept too, in packs of one scenario's files (stats.rs `stats_packs`), so
    /// no report needs the folder again. Other paths are left out. Answers how many of each it kept.
    pub fn add_kovaak_files(&self, body: &[u8]) -> Answer<Value> {
        let kovaak =
            self.store().kovaak().ok_or_else(|| Failure::from("KovaaK's files are not kept here".to_string()))?;
        let (mut stats, mut scenarios, mut texts) = (Vec::new(), Vec::new(), Vec::new());
        for file in crate::batch::read(body)? {
            let size = file.bytes.len() as u64;
            if let Some(name) = file.path.strip_prefix(STATS_PREFIX) {
                let run = super::stats::run_of_file(file.bytes);
                stats.push(StatsRow { name: name.to_string(), size, modified: file.modified, run });
                texts.push((name, file.bytes));
            } else if SCENARIO_PREFIXES.iter().any(|prefix| file.path.starts_with(prefix)) {
                let facts = aimview::scenario::facts(&aimview::scenario::text_of(file.bytes));
                scenarios.push(ScenarioRow { path: file.path.to_string(), size, modified: file.modified, facts });
            }
        }
        kovaak.add_stats_files(&stats)?;
        for pack in super::stats::stats_packs(texts) {
            kovaak.keep_stats_pack(&pack)?;
        }
        kovaak.add_scenarios(&scenarios)?;
        self.kovaak_changed()?;
        Ok(json!({ "stats": stats.len(), "scenarios": scenarios.len() }))
    }

    /// POST /api/cutoff_labels: a submit's detector labels the page made (batch.rs): its crops (train/<name>.npz),
    /// each kept in place of any of the same name, and its rows (rows.json, a list), written after the rows before as
    /// the native submit writes them, so a later submit's rows win. Answers how many crops and rows it kept.
    pub fn add_cutoff_labels(&self, body: &[u8]) -> Answer<Value> {
        let (mut crops, mut rows) = (0, Vec::<PageCutoffRow>::new());
        for file in crate::batch::read(body)? {
            if file.path == ROWS_FILE {
                rows = serde_json::from_slice(file.bytes)
                    .map_err(|error| Failure::bad(format!("{ROWS_FILE}: {error}")))?;
            } else if is_crop_path(file.path) {
                let item = Item::CutoffCrop(file.path);
                self.store()
                    .write(item, file.bytes)
                    .map_err(|error| format!("{}: {error}", self.store().name(item)))?;
                crops += 1;
            } else {
                return Err(Failure::bad(format!("not a cut-off label's file: {}", file.path)));
            }
        }
        let mut text = Vec::new();
        for row in &rows {
            text.extend(pyjson::to_vec(row, false));
            text.push(b'\n');
        }
        if !text.is_empty() {
            pyjson::append_text(self.store(), Item::CutoffRows, &text)?;
        }
        Ok(json!({ "crops": crops, "rows": rows.len() }))
    }

    /// POST /api/kovaak?changed=1: the page copied new KovaaK files, so the stats files and the scenarios are read
    /// again when next needed.
    pub fn kovaak_changed(&self) -> Answer<Value> {
        self.forget_stats();
        self.forget_facts();
        Ok(json!({ "changed": true }))
    }
}
