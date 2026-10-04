//! The browser build's own routes (api.rs): the page runs the review, the area finder and the cut-off's labels itself
//! and sends what they give, which is kept as the native review keeps it; it adds raw mouse logs, chooses the VODs
//! folder (a folder it mounted) and says when it copied new KovaaK files.

use std::path::{Path, PathBuf};

use aimview::areas::Found;
use serde::Deserialize;
use serde_json::{Value, json};

use super::names::free_name;
use super::reviews::{Job, keep_review};
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
/// as its mounted path, the model by name, the device by its name ("webgpu" or "wasm"); not the runs (the page splits
/// the recording itself).
pub(super) fn review_json(req: &Request, model: &str) -> Value {
    json!({
        "video": req.video, "model": model, "device": req.device.name(), "batch": req.batch, "cap": req.cap,
        "window": req.window, "areas": req.areas,
    })
}

/// Whether a job has not ended.
fn running(j: &Job) -> bool {
    j.stage != "done" && j.stage != "error"
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
        let p: PageProgress = serde_json::from_slice(body).map_err(|e| Failure::bad(format!("the progress: {e}")))?;
        let job = self.jobs.lock().map_err(|_| "the jobs are broken".to_string())?.get(id).cloned();
        let job = job.ok_or_else(|| Failure::missing(format!("no review of {id} is running")))?;
        let mut j = job.lock().map_err(|_| "the job is broken".to_string())?;
        if running(&j) {
            j.review = None;
            match (p.error, p.stage) {
                (Some(e), _) => (j.stage, j.error) = ("error".into(), Some(e)),
                (None, Some(stage)) if stage == "done" || stage == "error" => {
                    return Err(Failure::bad("a review ends with /api/reviewed, or an error"));
                }
                (None, Some(stage)) => (j.stage, j.done, j.total) = (stage, p.done, p.total),
                (None, None) => return Err(Failure::bad("the progress needs a stage or an error")),
            }
        }
        Ok(json!(*j))
    }

    /// POST /api/reviewed: the review the page ran, its files written as the native review's end writes them (in
    /// models/<model>/ of the recording's folder) and its job done. With no job running (the data move) it only
    /// writes the files.
    pub fn review_done(&self, id: &str, body: &[u8]) -> Answer<Value> {
        let r: PageReview = serde_json::from_slice(body).map_err(|e| Failure::bad(format!("the review: {e}")))?;
        if !r.tracks["frames"].is_array() {
            return Err(Failure::bad("the review: its tracks have no frames"));
        }
        let job = self.jobs.lock().map_err(|_| "the jobs are broken".to_string())?.get(id).cloned();
        let job = job.filter(|j| j.lock().is_ok_and(|j| running(&j)));
        let job_model = job.as_ref().and_then(|j| j.lock().ok().map(|j| j.model.clone()));
        let model = r.model.or(job_model).unwrap_or_else(|| self.model());
        if model.is_empty() || Path::new(&model).file_name().is_none_or(|f| f != model.as_str()) {
            return Err(Failure::bad(format!("not a model's name: {model}")));
        }
        let out = self.review_dir(id).join("models").join(&model);
        let outcome = keep_review(&out, &r.tracks, &r.readings, &r.hud, r.found.as_ref());
        let Some(job) = job else {
            outcome?;
            return Ok(json!(Job::new("done", &model)));
        };
        let mut j = job.lock().map_err(|_| "the job is broken".to_string())?;
        j.review = None;
        match outcome {
            Ok(()) => {
                (j.stage, j.done, j.total) = ("done".into(), 1, 1);
                j.seconds = r.seconds.map(|s| (s * 10.0).round() / 10.0);
                if r.device.is_some() {
                    j.device = r.device;
                }
                Ok(json!(*j))
            }
            Err(e) => {
                (j.stage, j.error) = ("error".into(), Some(e.clone()));
                Err(e.into())
            }
        }
    }

    /// POST /api/found: what the page's area finder found in the recording (the core's `Found`), kept as the native
    /// finder keeps it (finder.rs: `keep`); /api/find_areas then proposes from it.
    pub fn keep_found(&self, id: &str, body: &[u8]) -> Answer<Value> {
        let found: Found = serde_json::from_slice(body).map_err(|e| Failure::bad(format!("the found areas: {e}")))?;
        crate::finder::keep(&self.review_dir(id), &found)?;
        Ok(json!({ "id": id, "kept": true }))
    }

    /// POST /api/mouse_log: a raw mouse log the user added (its bytes), kept in the mouse folder as the desktop's
    /// logger leaves them. The same log again is kept once; another of the same name gets a free name.
    pub fn keep_mouse_log(&self, name: &str, body: &[u8]) -> Answer<Value> {
        let plain = Path::new(name).file_name().is_some_and(|f| f == name) && name.ends_with(".bin");
        if !plain {
            return Err(Failure::bad(format!("not a mouse log (.bin): {name}")));
        }
        let dir = &self.folders.mouse;
        crate::disk::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let mut p = dir.join(name);
        if crate::disk::read(&p).is_ok_and(|old| old == body) {
            return Ok(json!({ "saved": name }));
        }
        p = free_name(p);
        crate::disk::write(&p, body).map_err(|e| format!("{}: {e}", p.display()))?;
        Ok(json!({ "saved": p.file_name().map(|n| n.to_string_lossy().into_owned()) }))
    }

    /// POST /api/kovaak?changed=1: the page copied new KovaaK files, so the stats files and the scenarios are read again
    /// when next needed.
    pub fn kovaak_changed(&self) -> Answer<Value> {
        self.forget_stats();
        *self.facts.lock().unwrap_or_else(|e| e.into_inner()) = None;
        Ok(json!({ "changed": true }))
    }
}
