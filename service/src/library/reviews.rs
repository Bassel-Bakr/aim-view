//! A recording's reviews (each model's in models/<model>/ in its folder: tracks.json, readings.json, hud.json): the
//! review on show, the review jobs (each runs in a thread of its own; in the browser build the page runs it,
//! browser.rs), the user's run window and the report, worked out when it is shown (python/server.py: shown, analyse,
//! run, set_run, /api/report). In: /api/analyse, /api/job, /api/run, /api/tracks and /api/report. Out: the review's
//! files (review.rs's results), run.json (run_window.rs) and the answers.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use aimview::areas::Found;
use serde::Serialize;
use serde_json::{Value, json};

use super::{Answer, Library, modified, read_json, write_json};
use crate::review::{Request, TimeWindow};
use crate::run_window::{RunMarks, covers};

/// The bytes at the end of an old tracks.json (python/server.py's, kept in the recording's own folder) read for the
/// detector's name.
const OLD_TRACKS_TAIL_BYTES: u64 = 200;
/// The key an old tracks.json ends with when a model made it (the hand-written detector's has none).
const DETECTOR_KEY: &[u8] = b"\"detector\"";
/// The threads a computer needs for a review in two runs at once (each run decodes on its own).
const TWO_RUNS_THREADS: usize = 8;
/// A job's and the log's times are rounded to a tenth of a second.
const TENTHS_PER_SECOND: f64 = 10.0;

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

    /// Whether the job is still at work (not done, and not failed).
    pub(super) fn running(&self) -> bool {
        self.stage != "done" && self.stage != "error"
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

/// Writes a review's files in its folder `out` (models/<model> in the recording's): its tracks, the video's readings
/// and what the HUD read, and keeps what the area finder found (`finder::keep_with_review`). The native review's end
/// and the page's (/api/reviewed) both write them so.
pub(super) fn keep_review(
    out: &Path,
    tracks: &impl Serialize,
    readings: &impl Serialize,
    hud: &impl Serialize,
    found: Option<&Found>,
) -> Result<(), String> {
    write_json(&out.join("tracks.json"), tracks).map_err(|failure| failure.message)?;
    write_json(&out.join("readings.json"), readings).map_err(|failure| failure.message)?;
    write_json(&out.join("hud.json"), hud).map_err(|failure| failure.message)?;
    crate::finder::keep_with_review(out, found)
}

/// The model of an old tracks.json (python/server.py's, in the recording's own folder): "hand" when the hand-written
/// detector made it, else None (not recorded). The detector's name ends the file.
fn old_tracks_model(tracks: &Path) -> Option<String> {
    let mut end = Vec::new();
    if let Ok(mut file) = crate::disk::File::open(tracks) {
        let size = file.metadata().map_or(0, |metadata| metadata.len());
        let start = SeekFrom::Start(size.saturating_sub(OLD_TRACKS_TAIL_BYTES));
        let _ = file.seek(start).and_then(|_| file.read_to_end(&mut end));
    }
    let named = end.windows(DETECTOR_KEY.len()).any(|window| window == DETECTOR_KEY);
    (!named).then(|| "hand".to_string())
}

/// The folders in `models` (a recording's models/) that hold a review.
fn review_folders(models: &Path) -> impl Iterator<Item = PathBuf> {
    let folders = crate::disk::read_dir(models).into_iter().flatten().flatten().map(|entry| entry.path());
    folders.filter(|folder| crate::disk::is_file(folder.join("tracks.json")))
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
            return (old_tracks_model(&old), dir);
        }
        let changed = |folder: &PathBuf| modified(&folder.join("tracks.json"));
        let newest = review_folders(&models).max_by(|a, b| changed(a).total_cmp(&changed(b)));
        match newest {
            Some(dir) => {
                let name = dir.file_name().map(|name| name.to_string_lossy().into_owned());
                (Some(name.unwrap_or_default()), dir)
            }
            None => (Some(model), own),
        }
    }

    /// Whether the recording has a review.
    pub(crate) fn reviewed(&self, id: &str) -> bool {
        let dir = self.review_dir(id);
        crate::disk::is_file(dir.join("tracks.json")) || review_folders(&dir.join("models")).next().is_some()
    }

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
        let (shown, dir) = self.shown(id);
        if !again && crate::disk::is_file(dir.join("tracks.json")) && self.tracked_with_areas(id, &dir) {
            return Ok(json!(Job::new("done", &shown.unwrap_or_default())));
        }
        let model = self.model();
        let request = self.review_request(id, video, &model)?;
        #[cfg(not(feature = "native"))]
        let job = Job { review: Some(super::browser::review_json(&request, &model)), ..Job::new("starting", &model) };
        #[cfg(feature = "native")]
        let job = Job::new("starting", &model);
        let job = Arc::new(Mutex::new(job));
        jobs.insert(id.to_string(), job.clone());
        drop(jobs);
        let first = json!(*job.lock().map_err(|_| "the job is broken".to_string())?);
        #[cfg(feature = "native")]
        self.run_review(id, request, job, model);
        Ok(first)
    }

    /// What a new review of the recording by `model` takes: the scenario's target count, the runs to split it into,
    /// the user's run window (only its part of the video is tracked) and the recording's areas.
    fn review_request(&self, id: &str, video: PathBuf, model: &str) -> Answer<Request> {
        let facts = self.facts_of(&video);
        let cap = facts.as_ref().and_then(|facts| facts.targets).unwrap_or(0);
        let threads = std::thread::available_parallelism().map_or(1, |threads| threads.get());
        let runs = if threads >= TWO_RUNS_THREADS { 2 } else { 1 };
        // the user's run window: only its part of the video is tracked
        let window = RunMarks::read(&self.review_dir(id)).tracked(facts.and_then(|facts| facts.limit));
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
        })
    }

    /// Runs a review in a thread of its own, its progress, device and end kept in `job`.
    #[cfg(feature = "native")]
    fn run_review(&self, id: &str, request: Request, job: Arc<Mutex<Job>>, model: String) {
        use crate::review::{add_device, review};
        let out = self.review_dir(id).join("models").join(&model);
        let started = std::time::Instant::now();
        let id = id.to_string();
        std::thread::spawn(move || {
            let progress = |stage: &str, done: usize, total: usize| {
                if let Ok(mut job) = job.lock() {
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
            let outcome = reviewed.and_then(|reviewed| {
                keep_review(&out, &reviewed.tracks, &reviewed.readings, &reviewed.hud, reviewed.found.as_ref())
            });
            if let Ok(mut job) = job.lock() {
                // one line in the log for each review: the model, the device it ran on, and the time or the error
                let on = job.device.as_ref().map_or(String::new(), |device| format!(" on {device}"));
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
        Ok(json!(RunMarks::read(&self.review_dir(id))))
    }

    /// Keeps the run window ({start, end, length}; all null forgets it). The report reads it when it is shown; a review
    /// that tracked less of the video than the new window needs is made again.
    pub fn set_marks(self: &Arc<Self>, id: &str, body: &Value) -> Answer<Value> {
        let video = self.resolve(id)?;
        let marks = RunMarks::parse(body).map_err(super::Failure::bad)?;
        marks.save(&self.review_dir(id))?;
        let (shown, dir) = self.shown(id);
        let Some(tracks) = read_json::<Value>(&dir.join("tracks.json")) else {
            return Ok(json!(Job::new("none", "")));
        };
        let tracked: Option<TimeWindow> = serde_json::from_value(tracks["window"].clone()).unwrap_or(None);
        let limit = self.facts_of(&video).and_then(|facts| facts.limit);
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
        let run = Some(RunMarks::read(&self.review_dir(id)));
        let faint = Some(self.faint(id));
        let worked_out = crate::report::work_out(&dir, &video, stats.as_deref(), run, facts.as_ref(), faint)?;
        let Some(mut report) = worked_out else { return Ok(Value::Null) };
        report["review_model"] = json!(model);
        Ok(report)
    }
}
