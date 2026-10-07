//! A recording's reviews (each model's kept apart, store.rs: tracks, readings, what the HUD read, the kills' check):
//! the review on show, the review jobs (each runs in a thread of its own; in the browser build the page runs it,
//! browser.rs), the user's run window and the report, worked out when it is shown (python/retired/server.py: shown,
//! analyse, run, set_run, /api/report). In: /api/analyse, /api/job, /api/cancel, /api/run, /api/tracks and
//! /api/report. Out: the kept reviews (review.rs's results), the run window (run_window.rs) and the answers.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use aimview::areas::Found;
use aimview::kill_check::KillEvidence;
use serde::Serialize;
use serde_json::{Value, json};

use super::{Answer, Library, keep_json, read_kept};
use crate::review::{CANCELLED, Request, TimeWindow};
use crate::run_window::{RunMarks, covers};
use crate::store::{Item, Part, ReviewBy, Store};

/// A job's and the log's times are rounded to a tenth of a second.
const TENTHS_PER_SECOND: f64 = 10.0;

/// A review job: its stage, how far it is (frames), the device its detector runs on once it has loaded ("DirectML",
/// "CUDA" or "CPU"; "DirectML and CPU" when its runs' differ), and at the end its time or its error. A link's download
/// (links.rs) is a job too, marked `link`: its stage is "downloading" (megabytes), or "ffmpeg" or "yt-dlp" while they
/// are fetched. A job the user cancels ends as "cancelled".
#[derive(Clone, Serialize)]
pub struct Job {
    /// "starting", the review's stages ("looking", "tracking", "linking", "checking"), a download's, or at the end
    /// "done", "error" or "cancelled".
    pub(super) stage: String,
    /// How far the stage is: frames, or megabytes for a download.
    pub(super) done: usize,
    /// Of how many.
    pub(super) total: usize,
    /// The whole job's time in seconds, to a tenth, once it is done.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) seconds: Option<f64>,
    /// Why it failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) error: Option<String>,
    /// The model the review is by (empty for a link's download).
    pub(super) model: String,
    /// Where its detector runs, once it has loaded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) device: Option<String>,
    /// Whether it is a link's download.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub(super) link: bool,
    /// Set when the user cancels the job: the review's and the download's loops look at it.
    #[serde(skip)]
    pub(super) cancel: Arc<AtomicBool>,
    /// The browser build's: what the page is to review (browser.rs: `review_json`), until it reports progress.
    #[cfg(not(feature = "native"))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) review: Option<Value>,
}

impl Job {
    /// A job at `stage` by `model`, nothing done yet (0 of 1).
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
            cancel: Arc::new(AtomicBool::new(false)),
            #[cfg(not(feature = "native"))]
            review: None,
        }
    }

    /// A link's download, starting.
    #[cfg(feature = "native")]
    pub(super) fn download() -> Job {
        Job { link: true, ..Job::new("downloading", "") }
    }

    /// Whether the job is still at work (not done, failed or cancelled).
    pub(super) fn running(&self) -> bool {
        self.stage != "done" && self.stage != "error" && self.stage != CANCELLED
    }

    /// Whether the user cancelled it.
    pub(super) fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

/// Seconds rounded to a tenth, for a job's time and the log.
pub(super) fn to_tenths(seconds: f64) -> f64 {
    (seconds * TENTHS_PER_SECOND).round() / TENTHS_PER_SECOND
}

/// The seconds since `started`, rounded to a tenth, for a job's time and the log.
#[cfg(feature = "native")]
pub(super) fn seconds_since(started: std::time::Instant) -> f64 {
    to_tenths(started.elapsed().as_secs_f64())
}

/// A review's files: its tracks, the video's readings, what the HUD read and the check of the video's kills (null:
/// not checked).
pub(super) struct ReviewFiles<'a, T: Serialize, R: Serialize, H: Serialize> {
    /// The tracks (tracks.json).
    pub tracks: &'a T,
    /// The camera's turn and the countdown (readings.json).
    pub readings: &'a R,
    /// What the HUD read, or null (hud.json).
    pub hud: &'a H,
    /// The kill check's evidence (kills.json); None: not checked.
    pub kills: Option<&'a [KillEvidence]>,
}

/// Keeps a review by `model` of the recording `id`, and what the area finder found (`finder::keep_with_review`).
/// The native review's end and the page's (/api/reviewed) both keep them so; the kills' check is kept null when the
/// kills were not checked, so a review made again keeps no older check.
pub(super) fn keep_review<T: Serialize, R: Serialize, H: Serialize>(
    store: &dyn Store,
    id: &str,
    model: &str,
    files: &ReviewFiles<T, R, H>,
    found: Option<&Found>,
) -> Result<(), String> {
    let by = ReviewBy::Model(model.to_string());
    let item = |part: Part| Item::ReviewPart(id, &by, part);
    let message = |failure: super::Failure| failure.message;
    keep_json(store, item(Part::Tracks), files.tracks).map_err(message)?;
    keep_json(store, item(Part::Readings), files.readings).map_err(message)?;
    keep_json(store, item(Part::Hud), files.hud).map_err(message)?;
    keep_json(store, item(Part::Kills), &files.kills).map_err(message)?;
    crate::finder::keep_with_review(store, id, found)
}

impl Library {
    /// The review to show: (model, which): the chosen model's; else one python/retired/server.py made before reviews
    /// were kept per model (its model "hand" when the hand-written detector made it, else None: not recorded); else the
    /// newest by another model. With none, the chosen model's, for a new one.
    pub fn shown(&self, id: &str) -> (Option<String>, ReviewBy) {
        let model = self.model();
        let own = ReviewBy::Model(model.clone());
        if self.store().has(Item::ReviewPart(id, &own, Part::Tracks)) {
            return (Some(model), own);
        }
        if let Some(old_model) = self.store().old_review(id) {
            return (old_model, ReviewBy::Old);
        }
        let changed = |name: &String| {
            let by = ReviewBy::Model(name.clone());
            self.store().changed(Item::ReviewPart(id, &by, Part::Tracks)).unwrap_or(0.0)
        };
        let newest = self.store().models(id).into_iter().max_by(|a, b| changed(a).total_cmp(&changed(b)));
        match newest {
            Some(name) => (Some(name.clone()), ReviewBy::Model(name)),
            None => (Some(model), own),
        }
    }

    /// A part of one of the recording's reviews, as kept; None when it is not.
    pub(crate) fn review_part(&self, id: &str, by: &ReviewBy, part: Part) -> Option<Vec<u8>> {
        self.store().read(Item::ReviewPart(id, by, part)).ok().flatten()
    }

    /// Whether the recording has a review.
    pub(crate) fn reviewed(&self, id: &str) -> bool {
        self.store().reviewed(id)
    }

    /// The recording's last job as JSON (`Job`), or {stage: "none"} when it has none.
    pub fn job(&self, id: &str) -> Value {
        let jobs = self.jobs.lock().unwrap_or_else(PoisonError::into_inner);
        let job = jobs.get(id).and_then(|job| job.lock().ok().map(|job| json!(*job)));
        job.unwrap_or_else(|| json!({ "stage": "none" }))
    }

    /// Reviews a recording: with again, a new review by the chosen model; else the one on show, or a new one when
    /// there is none. The review runs in a thread of its own; `job` follows it. In the browser build the page runs it:
    /// the job waits as "starting" and carries the review to run (`review`, browser.rs: `review_json`) until the page
    /// reports progress.
    pub fn analyse(self: &Arc<Self>, id: &str, again: bool) -> Answer<Value> {
        let mut jobs = self.jobs.lock().map_err(|_| "the jobs are broken".to_string())?;
        if let Some(job) = jobs.get(id)
            && let Ok(job) = job.lock()
            && job.running()
        {
            return Ok(json!(*job));
        }
        let video = self.resolve(id)?;
        let (shown, by) = self.shown(id);
        if !again && self.store().has(Item::ReviewPart(id, &by, Part::Tracks)) && self.tracked_with_areas(id, &by) {
            return Ok(json!(Job::new("done", &shown.unwrap_or_default())));
        }
        let model = self.model();
        let request = self.review_request(id, video, &model)?;
        #[cfg(not(feature = "native"))]
        let job = Job { review: Some(super::browser::review_json(&request, &model)), ..Job::new("starting", &model) };
        #[cfg(feature = "native")]
        let job = Job::new("starting", &model);
        #[cfg(feature = "native")]
        let request = Request { cancel: Some(job.cancel.clone()), ..request };
        let job = Arc::new(Mutex::new(job));
        jobs.insert(id.to_string(), job.clone());
        drop(jobs);
        let first = json!(*job.lock().map_err(|_| "the job is broken".to_string())?);
        #[cfg(feature = "native")]
        self.run_review(id, request, job, model);
        Ok(first)
    }

    /// Cancels the recording's job (a review, or a link's download) while it runs: its stage "cancelled" at once,
    /// the review or download stopping at its next frame or line, keeping nothing (the review on show stays). In the
    /// browser build the page that runs the review sees the stage and stops its workers. Answers the job.
    pub fn cancel(&self, id: &str) -> Answer<Value> {
        let job = self.jobs.lock().map_err(|_| "the jobs are broken".to_string())?.get(id).cloned();
        let Some(job) = job else { return Ok(json!({ "stage": "none" })) };
        let mut state = job.lock().map_err(|_| "the job is broken".to_string())?;
        if state.running() {
            state.cancel.store(true, Ordering::Relaxed);
            state.stage = CANCELLED.into();
            #[cfg(not(feature = "native"))]
            {
                state.review = None;
            }
        }
        Ok(json!(*state))
    }

    /// What a new review of the recording by `model` takes: the scenario's target count, the runs to split it into,
    /// the user's run window (only its part of the video is tracked) and the recording's areas.
    fn review_request(&self, id: &str, video: PathBuf, model: &str) -> Answer<Request> {
        let facts = self.facts_of(&video);
        let cap = facts.as_ref().and_then(|facts| facts.targets).unwrap_or(0);
        let threads = std::thread::available_parallelism().map_or(1, |threads| threads.get());
        let runs = crate::review::parts_at_once(threads, self.config.gpu_frames);
        // the user's run window: only its part of the video is tracked
        let window = RunMarks::read(self.store(), id).tracked(facts.as_ref().and_then(|facts| facts.limit));
        Ok(Request {
            video,
            model: self.model_file(model),
            device: self.device(),
            batch: self.batch(self.device()),
            cap,
            runs,
            window,
            areas: self.exclude_boxes(id)?,
            keep_parts: None,
            gpu_frames: self.config.gpu_frames,
            gpu_share: 1.0,
            // without a stats file the report takes the kills from the video: check them in the frames round them
            kill_check: self.stats_path(id).is_none(),
            kind: facts.map(|facts| facts.kind),
            cancel: None,
        })
    }

    /// Runs a review in a thread of its own, its progress, device and end kept in `job`.
    #[cfg(feature = "native")]
    fn run_review(&self, id: &str, request: Request, job: Arc<Mutex<Job>>, model: String) {
        use crate::review::{add_device, review};
        let store = self.shared_store();
        let started = std::time::Instant::now();
        let id = id.to_string();
        std::thread::spawn(move || {
            let progress = |stage: &str, done: usize, total: usize| {
                if let Ok(mut job) = job.lock()
                    && job.running()
                {
                    (job.stage, job.done, job.total) = (stage.into(), done, total);
                }
            };
            let on_device = |device: &'static str| {
                if let Ok(mut job) = job.lock() {
                    add_device(job.device.get_or_insert_with(String::new), device);
                }
            };
            let reviewed = crate::ffmpeg::ensure(|megabytes, of| progress("ffmpeg", megabytes, of))
                .and_then(|()| review(&request, &progress, &on_device));
            let cancelled = || job.lock().is_ok_and(|job| job.cancelled());
            let outcome = reviewed.and_then(|reviewed| {
                if cancelled() {
                    return Err(CANCELLED.into());
                }
                let files = ReviewFiles {
                    tracks: &reviewed.tracks,
                    readings: &reviewed.readings,
                    hud: &reviewed.hud,
                    kills: reviewed.kills.as_deref(),
                };
                keep_review(&*store, &id, &model, &files, reviewed.found.as_ref())
            });
            if let Ok(mut job) = job.lock() {
                // one line in the log for each review: the model, the device it ran on, and the time or the error
                let on = job.device.as_ref().map_or(String::new(), |device| format!(" on {device}"));
                if job.cancelled() {
                    println!("the review of {id} was cancelled ({model}{on})");
                    job.stage = CANCELLED.into();
                    return;
                }
                match outcome {
                    Ok(()) => {
                        (job.stage, job.done, job.total) = ("done".into(), 1, 1);
                        let seconds = seconds_since(started);
                        job.seconds = Some(seconds);
                        println!("reviewed {id}: {model}{on}, {seconds} s");
                    }
                    Err(error) => {
                        println!("the review of {id} failed ({model}{on}): {error}");
                        (job.stage, job.error) = ("error".into(), Some(error));
                    }
                }
            }
        });
    }

    /// The user's run window for the recording (all three null when none is marked).
    pub fn marks(&self, id: &str) -> Answer<Value> {
        self.resolve(id)?;
        Ok(json!(RunMarks::read(self.store(), id)))
    }

    /// Keeps the run window ({start, end, length}; all null forgets it). The report reads it when it is shown; a review
    /// that tracked less of the video than the new window needs is made again.
    pub fn set_marks(self: &Arc<Self>, id: &str, body: &Value) -> Answer<Value> {
        let video = self.resolve(id)?;
        let marks = RunMarks::parse(body).map_err(super::Failure::bad)?;
        marks.save(self.store(), id)?;
        let (shown, by) = self.shown(id);
        let Some(tracks) = read_kept::<Value>(self.store(), Item::ReviewPart(id, &by, Part::Tracks)) else {
            return Ok(json!(Job::new("none", "")));
        };
        let tracked: Option<TimeWindow> = serde_json::from_value(tracks["window"].clone()).unwrap_or(None);
        let limit = self.facts_of(&video).and_then(|facts| facts.limit);
        if covers(tracked, marks.tracked(limit)) {
            return Ok(json!(Job::new("done", &shown.unwrap_or_default())));
        }
        self.analyse(id, true)
    }

    /// The shown review's tracks (tracks.json's bytes), or None.
    pub fn tracks(&self, id: &str) -> Option<Vec<u8>> {
        self.review_part(id, &self.shown(id).1, Part::Tracks)
    }

    /// The shown review's report, worked out by the core (report.rs) from its tracks and the stats file, or without
    /// one from what the HUD read, else from the video alone; None without a review.
    pub fn report(&self, id: &str) -> Answer<Value> {
        let video = self.resolve(id)?;
        let (model, by) = self.shown(id);
        let stats = self.stats_of(id, &video);
        let facts = self.facts_of(&video);
        let run = Some(RunMarks::read(self.store(), id));
        let faint = Some(self.faint(id));
        let parts = |part: Part| self.review_part(id, &by, part);
        let worked_out = crate::report::work_out(parts, &video, stats.as_deref(), run, facts.as_ref(), faint)?;
        let Some(mut report) = worked_out else { return Ok(Value::Null) };
        report["review_model"] = json!(model);
        Ok(report)
    }
}

/// The review jobs.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Layout};

    /// A running job, cancelled: its stage "cancelled" at once, no longer running, and its flag set for the review's
    /// loops; a recording with no job answers "none".
    #[test]
    fn a_cancelled_job_stays_cancelled() {
        let dir = std::env::temp_dir().join(format!("aimview-cancel-{}", std::process::id()));
        let library = Library::open(Config::new(dir.clone(), Layout::App, dir.join("models"))).unwrap();
        let job = Arc::new(Mutex::new(Job::new("tracking", "large_v13e4")));
        library.jobs.lock().unwrap().insert("a.mp4".into(), job.clone());
        let answer = library.cancel("a.mp4").unwrap();
        assert_eq!(answer["stage"], CANCELLED);
        let state = job.lock().unwrap();
        assert!(state.cancelled() && !state.running());
        drop(state);
        assert_eq!(library.cancel("b.mp4").unwrap()["stage"], "none");
        let _ = std::fs::remove_dir_all(dir);
    }
}
