//! Aim View's review core. It is built natively for the desktop app and as WebAssembly for the browser. The Python
//! code in `python/` stays the reference: each part ported here must give the same reports on every recording.

pub mod camera;
pub mod convert;
pub mod detect;
pub mod fixed;
pub mod geometry;
pub mod matching;
pub mod measure;
pub mod popup;
pub mod python;
pub mod review;
pub mod scenario;
pub mod scipy;
pub mod statistics;
pub mod stats_file;
pub mod summary;
pub mod track;
pub mod tracker;
pub mod tracking;

#[cfg(target_arch = "wasm32")]
pub mod wasm;
