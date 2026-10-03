//! Aim View's desktop app (lib.rs).

// no console window in a release build
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    aimview_desktop::run();
}
