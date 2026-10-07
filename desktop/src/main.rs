//! Aim View's desktop app: the executable, which starts the app (lib.rs `run`). In: nothing. Out: the app's window,
//! or the mouse logger's process (lib.rs, mouse.rs).

// no console window in a release build
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// Starts the app.
fn main() {
    aimview_desktop::run();
}
