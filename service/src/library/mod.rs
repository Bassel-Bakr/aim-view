//! The library: the user's recordings (the VODs folder and the uploads), KovaaK's stats files and scenarios, the models,
//! and each recording's reviews, kept in the data folder (config.rs: `Layout`). It answers what the review server
//! (python/server.py) answers (api.rs); the review itself runs natively (review.rs).
//!
//! settings.rs: what the user set, the models and the model pick. recordings.rs: the recordings list, a recording's
//! video and folder, uploads, the scenarios' facts. stats.rs: KovaaK's stats files and each recording's pairing with
//! one. reviews.rs: the review jobs, the review on show, the run window and the report. names.rs: file names and time
//! stamps. The areas (areas.rs), the faint-target cut-off (faint.rs), labelling (labels.rs) and the mouse logs'
//! measures (mouse.rs) are kept beside it.

mod names;
mod recordings;
mod reviews;
mod settings;
mod stats;

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;

use aimview::scenario::Facts;
use serde::{Deserialize, Serialize};

use crate::config::{Config, Folders};
pub use names::{local_stamp, parse_name, parse_stats_name, slug, stamp_seconds};
pub use reviews::Job;
use settings::Settings;
use stats::StatsIndex;

/// An error for the page: its message, and the HTTP status the API answers with.
#[derive(Debug)]
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

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Failure {}

pub type Answer<T> = Result<T, Failure>;

pub struct Library {
    config: Config,
    folders: Folders,
    settings: Mutex<Settings>,
    stats: Mutex<StatsIndex>,
    facts: Mutex<Option<Arc<HashMap<String, Facts>>>>,
    jobs: Mutex<HashMap<String, Arc<Mutex<Job>>>>,
}

pub(crate) fn modified(p: &Path) -> f64 {
    p.metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0.0, |d| d.as_secs_f64())
}

pub(crate) fn read_json<T: for<'a> Deserialize<'a>>(p: &Path) -> Option<T> {
    serde_json::from_slice(&std::fs::read(p).ok()?).ok()
}

pub(crate) fn write_json(p: &Path, v: &impl Serialize) -> Answer<()> {
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(p, serde_json::to_vec(v).map_err(|e| e.to_string())?).map_err(|e| e.to_string().into())
}

impl Library {
    /// The library `config` describes, with ffmpeg taken from where it says (for the whole process). Area examples kept
    /// before area kinds had ids are given ids (python/server.py does it at its start); when they cannot be read the
    /// library still opens, and says why. Upload bodies another process left in the uploads folder (a crash
    /// mid-upload) are removed. Fails when the data folder cannot be made.
    pub fn open(config: Config) -> Result<Arc<Library>, String> {
        let folders = config.folders();
        std::fs::create_dir_all(&folders.files).map_err(|e| format!("{}: {e}", folders.files.display()))?;
        crate::ffmpeg::set_source(config.ffmpeg.clone());
        let settings = Settings::read(&folders.files.join(settings::FILE));
        let lib = Arc::new(Library {
            config,
            folders,
            settings: Mutex::new(settings),
            stats: Mutex::default(),
            facts: Mutex::default(),
            jobs: Mutex::default(),
        });
        recordings::remove_stale_spools(&lib.uploads(), std::process::id());
        if let Err(e) = lib.fix_examples() {
            eprintln!("the area finder's examples: {}", e.message);
        }
        Ok(lib)
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn folders(&self) -> &Folders {
        &self.folders
    }

    /// One of the library's own files (settings.json, area_kinds.json...).
    pub(crate) fn file(&self, name: &str) -> PathBuf {
        self.folders.files.join(name)
    }
}
