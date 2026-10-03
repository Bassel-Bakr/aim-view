//! Aim View's desktop app: the Angular app (ui/, its desktop build) in a Tauri 2 window, and the review run natively
//! (review.rs: ffmpeg's frames, the core, and the detector on the GPU).

pub mod detector;
pub mod review;
pub mod video;

/// Opens the app's window.
pub fn run() {
    tauri::Builder::default().run(tauri::generate_context!()).expect("Aim View could not start");
}
