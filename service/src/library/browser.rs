//! The browser build's own routes (api.rs): the page runs the review, the area finder and the cut-off's labels itself
//! and sends what they give, which is kept as the native review keeps it; it adds raw mouse logs, chooses the VODs
//! folder (a folder it mounted) and says when it copied new KovaaK files.

use std::path::{Path, PathBuf};

use aimview::areas::Found;
use serde::Deserialize;
use serde_json::{Value, json};

use super::names::free_name;
use super::reviews::{Job, ReviewFiles, keep_review, to_tenths};
use super::{Answer, Failure, Library};
use crate::review::Request;

/// A review the page ran (POST /api/reviewed): the files the native review writes, the area finder's find in the key
/// frames, its time (seconds) and device ("WebGPU", "WebAssembly"), and the model it ran (else the job's).
#[derive(Deserialize)]
struct PageReview {
    tracks: Value,
    readings: Value,
    #[serde(default)]
    hud: Value,
    #[serde(default)]
    found: Option<Found>,
    #[serde(default)]
    seconds: Option<f64>,
    #[serde(default)]
    device: Option<String>,
    #[serde(default)]
    model: Option<String>,
}

/// How far a review the page runs is (POST /api/job), or its error.
#[derive(Deserialize)]
struct PageProgress {
    #[serde(default)]
    stage: Option<String>,
    #[serde(default)]
    done: usize,
    #[serde(default)]
    total: usize,
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

    /// POST /api/kovaak?changed=1: the page copied new KovaaK files, so the stats files and the scenarios are read
    /// again when next needed.
    pub fn kovaak_changed(&self) -> Answer<Value> {
        self.forget_stats();
        *self.facts.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        Ok(json!({ "changed": true }))
    }
}
