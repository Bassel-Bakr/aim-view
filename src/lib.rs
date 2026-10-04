//! Aim View's review core. It is built natively for the desktop app and as WebAssembly for the browser. The Python
//! code in `python/` stays the reference: each part ported here must give the same reports on every recording.

pub mod areas;
pub mod camera;
pub mod convert;
pub mod detect;
pub mod faint;
pub mod fixed;
pub mod geometry;
pub mod hud;
pub mod matching;
pub mod measure;
pub mod model;
pub mod optional_fields;
pub mod mouse;
pub mod popup;
pub mod py_random;
pub mod python;
pub mod reload;
pub mod review;
pub mod scenario;
pub mod scipy;
pub mod statistics;
pub mod stats_file;
pub mod summary;
pub mod track;
pub mod tracker;
pub mod tracking;
#[cfg(feature = "ts")]
pub mod typescript;
pub mod what_if;

#[cfg(target_arch = "wasm32")]
pub mod wasm;
