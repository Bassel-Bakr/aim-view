//! Aim View's review service: the review server's API (python/server.py) over a library of recordings, with the review
//! run natively (ffmpeg's frames, the core, and the detector on the GPU). The desktop app (desktop/) and the HTTP
//! server (server/) serve it: each opens a `Library` from a `Config` and answers requests with `api::handle`. Python's
//! scripts use the library and the native review through aimview-tool (src/bin/aimview-tool.rs).
//!
//! config.rs: what a library needs to know. api.rs: the API's routes. library/: the recordings, their stats files, the
//! settings and models, the reviews and their reports. areas.rs, finder.rs: the areas a review leaves out and the area
//! finder. faint.rs: the faint-target cut-off. labels.rs: the labelling queues. mouse.rs: the mouse logs' measures.
//! review.rs, detector.rs, video.rs, ffmpeg.rs: the native review. report.rs: the report the core works out.
//! run_window.rs: the user's run window. pyjson.rs, npz.rs: files as Python writes them. ytdlp.rs: yt-dlp, for
//! recordings added from a link.

pub mod api;
pub mod areas;
pub mod config;
pub mod detector;
pub mod faint;
pub mod ffmpeg;
pub mod finder;
pub mod labels;
pub mod library;
pub mod mouse;
pub mod npz;
pub mod pyjson;
pub mod report;
pub mod review;
pub mod run_window;
pub mod video;
pub mod ytdlp;

pub use api::{ApiRequest, ApiResponse, handle};
pub use config::{Config, Device, Ffmpeg, Folders, Layout};
pub use library::{Answer, Failure, Library};
