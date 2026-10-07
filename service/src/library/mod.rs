//! The library: the user's recordings (the VODs folder and the uploads), KovaaK's stats files and scenarios, the
//! models, and each recording's reviews, kept in the data folder (config.rs: `Layout`). It answers what the review
//! server (python/server.py) answers (api.rs); the review itself runs natively (review.rs).
//!
//! settings.rs: what the user set, the models and the model pick. recordings.rs: the recordings list, a recording's
//! video and folder, uploads, the scenarios' facts. stats.rs: KovaaK's stats files and each recording's pairing with
//! one. reviews.rs: the review jobs, the review on show, the run window and the report. names.rs: file names and time
//! stamps. links.rs: recordings added from a link (yt-dlp). The areas (areas.rs), the faint-target cut-off
//! (faint.rs), labelling (labels.rs) and the mouse logs' measures (mouse.rs) are kept beside it.
//!
//! In: the library's `Config` and the API's requests (api.rs). Out: the answers, and what the library keeps
//! (store.rs: today the files in the data folder).

#[cfg(not(feature = "native"))]
mod browser;
#[cfg(feature = "native")]
mod links;
mod names;
mod recordings;
mod reviews;
mod settings;
mod stats;

use std::collections::HashMap;
use std::fmt;
use std::path::Path;
use std::sync::{Arc, Mutex};

use aimview::scenario::Facts;
use serde::{Deserialize, Serialize};

use crate::config::{Config, Folders};
use crate::store::{Files, Item, Store};
pub use names::{local_stamp, parse_name, parse_stats_name, slug, stamp_seconds};
pub(crate) use recordings::is_upload;
pub use reviews::Job;
use settings::Settings;
use stats::StatsIndex;

/// An error for the page: its message, and the HTTP status the API answers with.
#[derive(Debug)]
pub struct Failure {
    pub status: u16,
    pub message: String,
}

/// The status of an answer that needs the page's area finder first (the browser build's /api/find_areas).
pub const FOUND_NEEDED: u16 = 409;
/// The statuses of a failure: something not found, a request that cannot be answered as it is, and any other error.
const NOT_FOUND: u16 = 404;
const BAD_REQUEST: u16 = 400;
const SERVER_ERROR: u16 = 500;

impl Failure {
    pub fn missing(what: impl Into<String>) -> Failure {
        Failure { status: NOT_FOUND, message: what.into() }
    }
    pub fn bad(what: impl Into<String>) -> Failure {
        Failure { status: BAD_REQUEST, message: what.into() }
    }
}

impl From<String> for Failure {
    fn from(message: String) -> Failure {
        Failure { status: SERVER_ERROR, message }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for Failure {}

pub type Answer<T> = Result<T, Failure>;

pub struct Library {
    config: Config,
    folders: Folders,
    /// Where the library keeps what it keeps.
    store: Arc<dyn Store>,
    settings: Mutex<Settings>,
    stats: Mutex<StatsIndex>,
    facts: Mutex<Option<Arc<HashMap<String, Facts>>>>,
    jobs: Mutex<HashMap<String, Arc<Mutex<Job>>>>,
    /// What links' qualities were read (links.rs), by link, kept for their download.
    #[cfg(feature = "native")]
    links: Mutex<HashMap<String, crate::ytdlp::LinkInfo>>,
}

/// A file's time of change in seconds since 1970; 0 when it has none.
pub(crate) fn modified(path: &Path) -> f64 {
    crate::disk::metadata(path).ok().and_then(|metadata| metadata.modified()).unwrap_or(0.0)
}

/// A JSON file's value; None when it is missing or not that value.
pub(crate) fn read_json<T: for<'a> Deserialize<'a>>(path: &Path) -> Option<T> {
    serde_json::from_slice(&crate::disk::read(path).ok()?).ok()
}

/// A kept item's JSON value; None when nothing is kept or it is not that value.
pub(crate) fn read_kept<T: for<'a> Deserialize<'a>>(store: &dyn Store, item: Item<'_>) -> Option<T> {
    serde_json::from_slice(&store.read(item).ok()??).ok()
}

/// Keeps a value as compact JSON.
pub(crate) fn keep_json(store: &dyn Store, item: Item<'_>, value: &impl Serialize) -> Answer<()> {
    let bytes = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    store.write(item, &bytes).map_err(|error| error.to_string().into())
}

impl Library {
    /// The library `config` describes, with ffmpeg taken from where it says (for the whole process). Area examples kept
    /// before area kinds had ids are given ids (python/server.py does it at its start); when they cannot be read the
    /// library still opens, and says why. Upload bodies another process left in the uploads folder (a crash
    /// mid-upload) are removed. Fails when the data folder cannot be made.
    pub fn open(config: Config) -> Result<Arc<Library>, String> {
        let folders = config.folders();
        crate::disk::create_dir_all(&folders.files).map_err(|error| format!("{}: {error}", folders.files.display()))?;
        #[cfg(feature = "native")]
        crate::ffmpeg::set_source(config.ffmpeg.clone());
        let store: Arc<dyn Store> = Arc::new(Files::new(folders.clone()));
        let settings = Settings::read(&*store);
        let library = Arc::new(Library {
            config,
            folders,
            store,
            settings: Mutex::new(settings),
            stats: Mutex::default(),
            facts: Mutex::default(),
            jobs: Mutex::default(),
            #[cfg(feature = "native")]
            links: Mutex::default(),
        });
        recordings::remove_stale_spools(&library.uploads(), crate::disk::process_id());
        if let Err(failure) = library.fix_examples() {
            eprintln!("the area finder's examples: {}", failure.message);
        }
        Ok(library)
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn folders(&self) -> &Folders {
        &self.folders
    }

    /// Where the library keeps what it keeps.
    pub(crate) fn store(&self) -> &dyn Store {
        &*self.store
    }

    /// The same, for a thread of its own (the native review's end, a cut-off's labels).
    #[cfg(feature = "native")]
    pub(crate) fn shared_store(&self) -> Arc<dyn Store> {
        self.store.clone()
    }
}
