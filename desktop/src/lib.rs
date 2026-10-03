//! Aim View's desktop app: the Angular app (ui/, its desktop build) in a Tauri 2 window. The window's server-mode
//! services talk to the app itself (api.rs: the review server's API over the `api` protocol), which keeps the library
//! (library.rs) and runs the review natively (review.rs: ffmpeg's frames, the core, and the detector on the GPU).

pub mod api;
pub mod detector;
pub mod ffmpeg;
pub mod library;
pub mod review;
pub mod video;

use std::sync::Arc;

use tauri::Manager;

/// Opens the app's window, with its library in the app's data folder and the models it ships with.
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let data = app.path().app_data_dir()?;
            ffmpeg::set_folder(app.path().app_local_data_dir()?.join("ffmpeg"));
            let models = app.path().resource_dir()?.join("models");
            let lib = Arc::new(library::Library::new(data, models));
            app.manage(lib);
            Ok(())
        })
        .register_asynchronous_uri_scheme_protocol("api", |ctx, request, responder| {
            let lib = ctx.app_handle().state::<Arc<library::Library>>().inner().clone();
            // each request in a thread of its own: a listing or a report must not hold up the window
            std::thread::spawn(move || responder.respond(api::handle(&lib, &request)));
        })
        .run(tauri::generate_context!())
        .expect("Aim View could not start");
}
