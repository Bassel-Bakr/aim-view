//! Aim View's desktop app: the Angular app (ui/, its desktop build) in a Tauri 2 window.

// no console window in a release build
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    tauri::Builder::default().run(tauri::generate_context!()).expect("Aim View could not start");
}
