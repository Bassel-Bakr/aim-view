//! Aim View's desktop app: the Angular app (ui/, its desktop build) in a Tauri 2 window. The window's server-mode
//! services talk to the app itself (protocol.rs: the review server's API over the `api` protocol), which the review
//! service answers (service/: the library in the app's data folder, and the review run natively). The app adds the
//! folder dialog and the raw mouse logger (mouse.rs). In: the app's folders (AimView in the user's roaming and local
//! app data, moved there from the folders the app's identifier named) and its resource folder, and the window's
//! requests. Out: the window, the library's files in the data folder, ffmpeg and the window's own data in the local
//! data folder, and the mouse logs.

#[cfg(all(debug_assertions, windows))]
mod dev_window;
#[cfg(target_os = "linux")]
pub mod media;
pub mod mouse;
pub mod protocol;
#[cfg(target_os = "linux")]
mod video_server;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use aimview_service::{Config, Ffmpeg, Layout, Library};
use tauri::Manager;
use tauri::utils::config::{AppDirectoriesOverride, AppDirectoryOverrides};
use tauri_plugin_dialog::DialogExt;

/// The name of the app's folders in the user's roaming and local app data, in place of its identifier (Tauri's
/// default), which names them for code rather than for people.
const FOLDER_NAME: &str = "AimView";

/// Opens the app's window, with its library in the app's data folder and the models it ships with.
pub fn run() {
    // the mouse logger the app starts runs this executable in a process of its own (mouse.rs)
    if let Some(code) = mouse::child_main() {
        std::process::exit(code);
    }
    let mut context = tauri::generate_context!();
    let identifier = context.config().identifier.clone();
    context.config_mut().app.app_directories_override = app_folders(&identifier);
    let builder = tauri::Builder::default().plugin(tauri_plugin_dialog::init());
    // Linux: the videos over HTTP on this computer (video_server.rs), its address in the page before it loads
    #[cfg(target_os = "linux")]
    let (builder, mut videos) = match video_server::VideoServer::bind() {
        Ok(server) => match server.base() {
            Ok(base) => (builder.plugin(video_base_plugin(&base)), Some(server)),
            Err(error) => {
                eprintln!("no video server, so no video plays: {error}");
                (builder, None)
            }
        },
        Err(error) => {
            eprintln!("no video server, so no video plays: {error}");
            (builder, None)
        }
    };
    builder
        .setup(move |app| {
            let data = app.path().app_data_dir()?;
            mouse::set_folder(data.join("mouse"));
            // the VODs folder the user chooses in the app (settings.json), KovaaK's where Steam keeps them
            let config = Config {
                ffmpeg: Ffmpeg::Download(app.path().app_local_data_dir()?.join("ffmpeg")),
                ..Config::new(data, Layout::App, app.path().resource_dir()?.join("models"))
            };
            let lib = Library::open(config)?;
            #[cfg(target_os = "linux")]
            if let Some(server) = videos.take() {
                server.serve(lib.clone());
            }
            app.manage(lib);
            #[cfg(all(debug_assertions, windows))]
            if let Some(window) = app.get_webview_window("main") {
                dev_window::show_minimized(&window);
            }
            Ok(())
        })
        .register_asynchronous_uri_scheme_protocol("api", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let lib = app.state::<Arc<Library>>().inner().clone();
            // each request in a thread of its own: a listing or a report must not hold up the window
            std::thread::spawn(move || {
                let pick_folder = |title: &str| {
                    let folder = app.dialog().file().set_title(title).blocking_pick_folder()?;
                    folder.into_path().ok()
                };
                let window = protocol::Window { set_theme: &|theme| app.set_theme(theme), pick_folder: &pick_folder };
                responder.respond(protocol::handle(&lib, &request, &window));
            });
        })
        .run(context)
        .expect("Aim View could not start");
}

/// A plugin that only puts the video server's address in the page before it loads (`window.aimviewVideoBase`, which
/// ui/src/app/modes/tauri/desktop-recordings.ts reads).
#[cfg(target_os = "linux")]
fn video_base_plugin<R: tauri::Runtime>(base: &str) -> tauri::plugin::TauriPlugin<R> {
    let script = format!("window.aimviewVideoBase = {};", serde_json::Value::from(base));
    tauri::plugin::Builder::new("video-base").js_init_script(script).build()
}

/// The app's folders: FOLDER_NAME in the user's roaming app data (the library, the mouse logs) and in the local app
/// data (ffmpeg, the window's own data). Each folder an earlier version kept under the identifier is moved there
/// first, once; when it cannot be moved (the app runs already), the app goes on using it. None when Windows names no
/// app data folders (Tauri's defaults then).
fn app_folders(identifier: &str) -> Option<AppDirectoriesOverride> {
    let data = own_folder(&dirs::data_dir()?, identifier);
    let local_data = own_folder(&dirs::data_local_dir()?, identifier);
    Some(AppDirectoriesOverride::Directories(AppDirectoryOverrides {
        config: Some(data.clone()),
        data: Some(data),
        cache: Some(local_data.clone()),
        log: Some(local_data.join("logs")),
        local_data: Some(local_data),
    }))
}

/// The app's folder in `base`: FOLDER_NAME, with the identifier's folder moved into it when only that one is there;
/// the identifier's folder when the move fails, so nothing the user has is left behind.
fn own_folder(base: &Path, identifier: &str) -> PathBuf {
    let (old, new) = (base.join(identifier), base.join(FOLDER_NAME));
    if !old.is_dir() || new.exists() {
        return new;
    }
    match std::fs::rename(&old, &new) {
        Ok(()) => new,
        Err(error) => {
            eprintln!("{} could not move to {}: {error}", old.display(), new.display());
            old
        }
    }
}

/// The app's folders under their own name.
#[cfg(test)]
mod tests {
    use super::*;

    /// The identifier's folder moves to FOLDER_NAME with what it holds; once FOLDER_NAME is there, an identifier's
    /// folder found beside it is left as it is, and with neither the app starts in FOLDER_NAME.
    #[test]
    fn the_identifiers_folder_moves_to_the_apps_name() {
        let base = std::env::temp_dir().join(format!("aimview-folders-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("app.test").join("reviews")).unwrap();
        std::fs::write(base.join("app.test").join("settings.json"), b"{}").unwrap();
        assert_eq!(own_folder(&base, "app.test"), base.join(FOLDER_NAME));
        assert!(
            base.join(FOLDER_NAME).join("settings.json").is_file() && base.join(FOLDER_NAME).join("reviews").is_dir()
        );
        assert!(!base.join("app.test").exists(), "moved, not copied");
        std::fs::create_dir_all(base.join("app.test")).unwrap();
        assert_eq!(own_folder(&base, "app.test"), base.join(FOLDER_NAME));
        assert!(base.join("app.test").is_dir(), "left as it is");
        assert_eq!(own_folder(&base, "app.other"), base.join(FOLDER_NAME));
        std::fs::remove_dir_all(&base).unwrap();
    }
}
