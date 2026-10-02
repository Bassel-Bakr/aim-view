//! aimview's review core. It is built natively for the desktop app and as WebAssembly for the browser. The Python
//! code in `python/` stays the reference: each part ported here must give the same reports on every recording.

pub mod convert;
pub mod detect;
pub mod fixed;
pub mod geometry;
pub mod popup;
pub mod python;
pub mod scipy;
pub mod track;

#[cfg(target_arch = "wasm32")]
pub mod wasm;
