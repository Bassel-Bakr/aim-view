//! Aim View's desktop app: the Angular app (ui/, its desktop build) in a Tauri 2 window. The window's server-mode
//! services talk to the app itself (protocol.rs: the review server's API over the `api` protocol), which the review
//! service answers (service/: the library in the app's data folder, and the review run natively). The app adds the
//! folder dialog and the raw mouse logger (mouse.rs).

pub mod mouse;
pub mod protocol;

use std::path::Path;
use std::sync::Arc;

use aimview_service::config::{KOVAAK_DEFAULT, kovaak_scenarios, kovaak_stats};
use aimview_service::{Config, Device, Ffmpeg, Layout, Library};
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
            mouse::set_folder(data.join("mouse"));
            let kovaak = Path::new(KOVAAK_DEFAULT);
            let config = Config {
                data,
                layout: Layout::App,
                // the folder the user chooses in the app (settings.json)
                vods: None,
                stats: kovaak_stats(kovaak),
                scenarios: kovaak_scenarios(kovaak),
                models: app.path().resource_dir()?.join("models"),
                device: Device::Auto,
                ffmpeg: Ffmpeg::Download(app.path().app_local_data_dir()?.join("ffmpeg")),
                // frames decoded and converted on the GPU where the video allows it: the same reviews, less CPU
                gpu_frames: true,
            };
            app.manage(Library::open(config)?);
            Ok(())
        })
        .register_asynchronous_uri_scheme_protocol("api", |ctx, request, responder| {
            let lib = ctx.app_handle().state::<Arc<Library>>().inner().clone();
            // each request in a thread of its own: a listing or a report must not hold up the window
            std::thread::spawn(move || responder.respond(protocol::handle(&lib, &request)));
        })
        .run(tauri::generate_context!())
        .expect("Aim View could not start");
}
