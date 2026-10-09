//! Aim View's review core: it finds the targets in a recording's frames, follows them, and measures the aim. It is
//! built natively for the service (service/: the desktop app and the review server) and as WebAssembly for the browser
//! (src/wasm.rs). It began as a port of the old Python review (python/retired/review.py, retired on 2026-10-04); the
//! parity tests (tests/) still compare it with that review's stored outputs (test_out/parity/), and KovaaK's stats
//! files are the ground truth for new work.
//!
//! In: the frames the hosts decode, the detector's outputs, KovaaK's stats files and scenarios. Out: the tracks and
//! the reports, as JSON. Each module's header says where its own data comes from and goes.

pub mod areas;
pub mod camera;
pub mod capped;
pub mod convert;
pub mod dates;
pub mod detect;
pub mod faint;
pub mod fixed;
pub mod geometry;
pub mod hud;
pub mod kill_check;
#[cfg(not(target_arch = "wasm32"))]
pub mod local_config;
pub mod matching;
pub mod measure;
pub mod model;
pub mod mouse;
pub mod optional_fields;
pub mod popup;
pub mod py_random;
pub mod python;
pub mod reload;
pub mod review;
pub mod scenario;
pub mod scipy;
pub mod session;
pub mod shapes;
pub mod statistics;
pub mod stats_file;
pub mod summary;
pub mod track;
pub mod track_checks;
pub mod tracker;
pub mod tracking;
#[cfg(feature = "ts")]
pub mod typescript;
pub mod what_if;

#[cfg(target_arch = "wasm32")]
pub mod wasm;
