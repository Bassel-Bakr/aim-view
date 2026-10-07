//! The browser build's own routes (api.rs): the page runs the review and the area finder itself and sends what they
//! give, which is kept as the native review keeps it; it adds raw mouse logs, chooses the VODs folder (a folder it
//! mounted) and sends KovaaK's files the user chose, read once. In: /api/job (POST), /api/reviewed, /api/found,
//! /api/mouse_log, /api/folder, /api/kovaak and /api/kovaak_files. Out: the reviews, found areas, mouse logs, settings
//! and KovaaK's runs and scenario facts kept, and the jobs' state.

use std::path::{Path, PathBuf};

use aimview::areas::Found;
use serde::Deserialize;
use serde_json::{Value, json};

use super::names::free_name;
use super::reviews::{Job, ReviewFiles, keep_review, to_tenths};
use super::{Answer, Failure, Library};
use crate::review::Request;
use crate::store::{ScenarioRow, StatsRow};

/// Where a stats file is in /kovaak (POST /api/kovaak_files).
const STATS_PREFIX: &str = "stats/";
/// Where scenario files are in /kovaak: the user's, and the workshop's (one folder per item).
const SCENARIO_PREFIXES: [&str; 2] = ["scenarios/", "workshop/"];
/// A u32's bytes in a batch.
const U32_BYTES: usize = 4;
/// An f64's bytes in a batch.
const F64_BYTES: usize = 8;

/// One file of a batch the page sends (POST /api/kovaak_files).
struct BatchFile<'a> {
    /// Its path in /kovaak (stats/<name>, scenarios/<name>.sce, workshop/<item>/<name>.sce).
    path: &'a str,
    /// Its time of change in seconds since 1970.
    modified: f64,
    /// Its bytes.
    bytes: &'a [u8],
}

/// A batch's files: each [u32 path length][path, UTF-8][f64 time of change][u32 length][bytes], little-endian; a
/// batch that ends early or names a path that is not UTF-8 is refused (400).
fn batch_files(body: &[u8]) -> Answer<Vec<BatchFile<'_>>> {
    let mut rest = body;
    let mut files = Vec::new();
    while !rest.is_empty() {
        let path_len = take_u32(&mut rest)?;
        let path =
            std::str::from_utf8(take(&mut rest, path_len)?).map_err(|_| Failure::bad("a path that is not UTF-8"))?;
        let modified = f64::from_le_bytes(take(&mut rest, F64_BYTES)?.try_into().unwrap_or_default());
        let len = take_u32(&mut rest)?;
        files.push(BatchFile { path, modified, bytes: take(&mut rest, len)? });
    }
    Ok(files)
}

/// The first `len` bytes of `rest`, which then starts after them; a batch that ends early is refused (400).
fn take<'a>(rest: &mut &'a [u8], len: usize) -> Answer<&'a [u8]> {
    if rest.len() < len {
        return Err(Failure::bad("the batch of KovaaK's files ends early"));
    }
    let (part, after) = rest.split_at(len);
    *rest = after;
    Ok(part)
}

/// A little-endian u32 from the start of `rest` (see `take`).
fn take_u32(rest: &mut &[u8]) -> Answer<usize> {
    Ok(u32::from_le_bytes(take(rest, U32_BYTES)?.try_into().unwrap_or_default()) as usize)
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
    /// ones: {stats: [[name, size, time]], scenarios: [[path, size, time]]} (times in seconds since 1970).
    pub fn kovaak_files(&self) -> Answer<Value> {
        let kovaak =
            self.store().kovaak().ok_or_else(|| Failure::from("KovaaK's files are not kept here".to_string()))?;
        let stats = kovaak.stats_files().map_err(|error| error.to_string())?;
        let scenarios = kovaak.scenarios().map_err(|error| error.to_string())?;
        Ok(json!({
            "stats": stats.iter().map(|row| json!([row.name, row.size, row.modified])).collect::<Vec<_>>(),
            "scenarios": scenarios.iter().map(|row| json!([row.path, row.size, row.modified])).collect::<Vec<_>>(),
        }))
    }

    /// POST /api/kovaak_files: a batch of KovaaK's files the user chose, read once (`batch_files`): each stats file's
    /// run and each scenario file's facts are kept, each kind in one transaction, and both are read again when next
    /// needed; the whole text is kept too of each stats file one of the user's recordings pairs with (by name and
    /// time), so its report needs the folder no more. Other paths are left out. Answers how many of each it kept.
    pub fn add_kovaak_files(&self, body: &[u8]) -> Answer<Value> {
        let kovaak =
            self.store().kovaak().ok_or_else(|| Failure::from("KovaaK's files are not kept here".to_string()))?;
        let (mut stats, mut scenarios, mut texts) = (Vec::new(), Vec::new(), Vec::new());
        let recorded = self.recorded_runs();
        for file in batch_files(body)? {
            let size = file.bytes.len() as u64;
            if let Some(name) = file.path.strip_prefix(STATS_PREFIX) {
                let run = super::stats::run_of_file(file.bytes);
                stats.push(StatsRow { name: name.to_string(), size, modified: file.modified, run });
                if Library::pairs_with(&recorded, name) {
                    texts.push((name, file.bytes));
                }
            } else if SCENARIO_PREFIXES.iter().any(|prefix| file.path.starts_with(prefix)) {
                let facts = aimview::scenario::facts(&aimview::scenario::text_of(file.bytes));
                scenarios.push(ScenarioRow { path: file.path.to_string(), size, modified: file.modified, facts });
            }
        }
        kovaak.add_stats_files(&stats).map_err(|error| error.to_string())?;
        for (name, text) in texts {
            kovaak.keep_stats_csv(name, text).map_err(|error| error.to_string())?;
        }
        kovaak.add_scenarios(&scenarios).map_err(|error| error.to_string())?;
        self.kovaak_changed()?;
        Ok(json!({ "stats": stats.len(), "scenarios": scenarios.len() }))
    }

    /// POST /api/kovaak?changed=1: the page copied new KovaaK files, so the stats files and the scenarios are read
    /// again when next needed.
    pub fn kovaak_changed(&self) -> Answer<Value> {
        self.forget_stats();
        *self.facts.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        Ok(json!({ "changed": true }))
    }
}
