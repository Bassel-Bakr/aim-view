//! Aim View's desktop app: the Angular app (ui/, its desktop build) in a Tauri 2 window. The window's server-mode
//! services talk to the app itself (api.rs: the review server's API over the `api` protocol), which keeps the library
//! (library.rs) and runs the review natively (review.rs: ffmpeg's frames, the core, and the detector on the GPU).

pub mod api;
pub mod areas;
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

use std::sync::Arc;

use tauri::Manager;

/// Opens the app's window, with its library in the app's data folder and the models it ships with.
pub fn run() {
    // the mouse logger the app starts runs this executable in a process of its own (mouse.rs)
    if let Some(code) = mouse::child_main() {
        std::process::exit(code);
    }
    tauri::Builder::default()
        .setup(|app| {
            let data = app.path().app_data_dir()?;
            ffmpeg::set_folder(app.path().app_local_data_dir()?.join("ffmpeg"));
            let models = app.path().resource_dir()?.join("models");
            mouse::set_folder(data.join("mouse"));
            let lib = Arc::new(library::Library::new(data, models));
            if let Err(e) = lib.fix_examples() {
                eprintln!("the area finder's examples: {}", e.message);
            }
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
