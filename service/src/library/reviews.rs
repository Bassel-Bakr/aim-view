//! A recording's reviews (each model's in models/<model>/ in its folder: tracks.json, readings.json, hud.json): the
//! review on show, the review jobs (each runs in a thread of its own; in the browser build the page runs it, browser.rs),
//! the user's run window and the report, worked out when it is shown (python/server.py: shown, analyse, run, set_run,
//! /api/report).

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use aimview::areas::Found;
use serde::Serialize;
use serde_json::{Value, json};

use super::{Answer, Library, modified, read_json, write_json};
use crate::review::{Request, TimeWindow};
use crate::run_window::{RunMarks, covers};

/// A review job: its stage, how far it is (frames), the device its detector runs on once it has loaded ("DirectML",
/// "CUDA" or "CPU"; "DirectML and CPU" when its runs' differ), and at the end its time or its error. A link's download
/// (links.rs) is a job too, marked `link`: its stage is "downloading" (megabytes), or "ffmpeg" or "yt-dlp" while they
/// are fetched.
#[derive(Clone, Serialize)]
pub struct Job {
    pub(super) stage: String,
    pub(super) done: usize,
    pub(super) total: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) error: Option<String>,
    pub(super) model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) device: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub(super) link: bool,
    /// The browser build's: what the page is to review (browser.rs: `review_json`), until it reports progress.
    #[cfg(not(feature = "native"))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) review: Option<Value>,
}

impl Job {
    pub(crate) fn new(stage: &str, model: &str) -> Job {
        Job {
            stage: stage.into(),
            done: 0,
            total: 1,
            seconds: None,
            error: None,
            model: model.into(),
            device: None,
            link: false,
            #[cfg(not(feature = "native"))]
            review: None,
        }
    }

    /// A link's download, starting.
    #[cfg(feature = "native")]
    pub(super) fn download() -> Job {
        Job { link: true, ..Job::new("downloading", "") }
    }
}

/// Writes a review's files in its folder `out` (models/<model> in the recording's): its tracks, the video's readings
/// and what the HUD read, and keeps what the area finder found (`finder::keep_with_review`). The native review's end and
/// the page's (/api/reviewed) both write them so.
pub(super) fn keep_review(
    out: &Path,
    tracks: &impl Serialize,
    readings: &impl Serialize,
    hud: &impl Serialize,
    found: Option<&Found>,
) -> Result<(), String> {
    write_json(&out.join("tracks.json"), tracks).map_err(|f| f.message)?;
    write_json(&out.join("readings.json"), readings).map_err(|f| f.message)?;
    write_json(&out.join("hud.json"), hud).map_err(|f| f.message)?;
    crate::finder::keep_with_review(out, found)
}

impl Library {
    /// The review to show: (model, folder): the chosen model's; else one python/server.py made before reviews were
    /// kept per model (tracks.json in the recording's own folder; its model "hand" when the hand-written detector made
    /// it, else None: not recorded); else the newest by another model. With none, the chosen model's folder for a new
    /// one.
    pub fn shown(&self, id: &str) -> (Option<String>, PathBuf) {
        let model = self.model();
        let dir = self.review_dir(id);
        let models = dir.join("models");
        let own = models.join(&model);
        if crate::disk::is_file(own.join("tracks.json")) {
            return (Some(model), own);
        }
        let old = dir.join("tracks.json");
        if crate::disk::is_file(&old) {
            // the detector's name ends the file; the hand-written detector's has none
            let mut end = Vec::new();
            if let Ok(mut f) = crate::disk::File::open(&old) {
                let size = f.metadata().map_or(0, |m| m.len());
                let _ = f.seek(SeekFrom::Start(size.saturating_sub(200))).and_then(|_| f.read_to_end(&mut end));
            }
            let named = end.windows(10).any(|w| w == b"\"detector\"");
            return ((!named).then(|| "hand".to_string()), dir);
        }
        let other = crate::disk::read_dir(&models)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| crate::disk::is_file(p.join("tracks.json")))
            .max_by(|a, b| modified(&a.join("tracks.json")).total_cmp(&modified(&b.join("tracks.json"))));
        match other {
            Some(dir) => (Some(dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()), dir),
            None => (Some(model), own),
        }
    }

    /// Whether the recording has a review.
    pub(crate) fn reviewed(&self, id: &str) -> bool {
        let dir = self.review_dir(id);
        crate::disk::is_file(dir.join("tracks.json"))
            || crate::disk::read_dir(dir.join("models")).into_iter().flatten().flatten().any(|e| crate::disk::is_file(e.path().join("tracks.json")))
    }

    pub fn job(&self, id: &str) -> Value {
        let jobs = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
        jobs.get(id).and_then(|j| j.lock().ok().map(|j| json!(*j))).unwrap_or_else(|| json!({ "stage": "none" }))
    }

    /// Reviews a recording: with again, a new review by the chosen model; else the one on show, or a new one when
    /// there is none. The review runs in a thread of its own; `job` follows it. In the browser build the page runs it:
    /// the job waits as "starting" and carries the review to run (`review`, browser.rs: `review_json`) until the page
    /// reports progress.
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
        if !again && crate::disk::is_file(dir.join("tracks.json")) && self.tracked_with_areas(id, &dir) {
            return Ok(json!(Job::new("done", &shown.unwrap_or_default())));
        }
        let model = self.model();
        let req = self.review_request(id, video, &model)?;
        #[cfg(not(feature = "native"))]
        let job = Arc::new(Mutex::new(Job { review: Some(super::browser::review_json(&req, &model)), ..Job::new("starting", &model) }));
        #[cfg(feature = "native")]
        let job = Arc::new(Mutex::new(Job::new("starting", &model)));
        jobs.insert(id.to_string(), job.clone());
        drop(jobs);
        let first = json!(*job.lock().map_err(|_| "the job is broken".to_string())?);
        #[cfg(feature = "native")]
        self.run_review(id, req, job, model);
        Ok(first)
    }

    /// What a new review of the recording by `model` takes: the scenario's target count, the runs to split it into,
    /// the user's run window (only its part of the video is tracked) and the recording's areas.
    fn review_request(&self, id: &str, video: PathBuf, model: &str) -> Answer<Request> {
        let facts = self.facts_of(&video);
        let cap = facts.as_ref().and_then(|f| f.targets).unwrap_or(0);
        let runs = if std::thread::available_parallelism().map_or(1, |n| n.get()) >= 8 { 2 } else { 1 };
        // the user's run window: only its part of the video is tracked
        let window = RunMarks::read(&self.review_dir(id)).tracked(facts.and_then(|f| f.limit));
        Ok(Request {
            video,
            model: self.model_file(model),
            device: self.device(),
            batch: self.batch(self.device()),
            cap,
            runs,
            window,
            areas: self.exclude_boxes(id)?,
        })
    }

    /// Runs a review in a thread of its own, its progress, device and end kept in `job`.
    #[cfg(feature = "native")]
    fn run_review(&self, id: &str, req: Request, job: Arc<Mutex<Job>>, model: String) {
        use crate::review::{add_device, review};
        let out = self.review_dir(id).join("models").join(&model);
        let started = std::time::Instant::now();
        let id = id.to_string();
        std::thread::spawn(move || {
            let progress = |stage: &str, done: usize, total: usize| {
                if let Ok(mut j) = job.lock() {
                    (j.stage, j.done, j.total) = (stage.into(), done, total);
                }
            };
            let on_device = |device: &'static str| {
                if let Ok(mut j) = job.lock() {
                    add_device(j.device.get_or_insert_with(String::new), device);
                }
            };
            let reviewed = crate::ffmpeg::ensure(|mb, of| progress("ffmpeg", mb, of)).and_then(|()| review(&req, &progress, &on_device));
            let outcome = reviewed.and_then(|r| keep_review(&out, &r.tracks, &r.readings, &r.hud, r.found.as_ref()));
            if let Ok(mut j) = job.lock() {
                // one line in the log for each review: the model, the device it ran on, and the time or the error
                let on = j.device.as_ref().map_or(String::new(), |d| format!(" on {d}"));
                match outcome {
                    Ok(()) => {
                        (j.stage, j.done, j.total) = ("done".into(), 1, 1);
                        let seconds = (started.elapsed().as_secs_f64() * 10.0).round() / 10.0;
                        j.seconds = Some(seconds);
                        println!("reviewed {id}: {model}{on}, {seconds} s");
                    }
                    Err(e) => {
                        println!("the review of {id} failed ({model}{on}): {e}");
                        (j.stage, j.error) = ("error".into(), Some(e));
                    }
                }
            }
        });
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
        let marks = RunMarks::parse(body).map_err(super::Failure::bad)?;
        marks.save(&self.review_dir(id))?;
        let (shown, dir) = self.shown(id);
        let Some(tracks) = read_json::<Value>(&dir.join("tracks.json")) else { return Ok(json!(Job::new("none", ""))) };
        let tracked: Option<TimeWindow> = serde_json::from_value(tracks["window"].clone()).unwrap_or(None);
        let limit = self.facts_of(&video).and_then(|f| f.limit);
        if covers(tracked, marks.tracked(limit)) {
            return Ok(json!(Job::new("done", &shown.unwrap_or_default())));
        }
        self.analyse(id, true)
    }

    /// The shown review's tracks (tracks.json), or None.
    pub fn tracks(&self, id: &str) -> Option<Vec<u8>> {
        crate::disk::read(self.shown(id).1.join("tracks.json")).ok()
    }

    /// The shown review's report, worked out by the core (report.rs) from its tracks and the stats file, or without
    /// one from what the HUD read, else from the video alone; None without a review.
    pub fn report(&self, id: &str) -> Answer<Value> {
        let video = self.resolve(id)?;
        let (model, dir) = self.shown(id);
        let stats = self.stats_of(id, &video);
        let facts = self.facts_of(&video);
        let run = RunMarks::read(&self.review_dir(id));
        let Some(mut report) = crate::report::work_out(&dir, &video, stats.as_deref(), Some(run), facts.as_ref(), Some(self.faint(id)))? else {
            return Ok(Value::Null);
        };
        report["review_model"] = json!(model);
        Ok(report)
    }
}
