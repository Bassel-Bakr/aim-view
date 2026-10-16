//! A development build's window kept out of the way: `bun run app` creates it hidden (tauri.dev.json) and this shows it
//! minimized without activating it, so a start, or a restart after a change, only adds it to the taskbar and never
//! covers or takes the focus from what is in front (a game). A release build's window opens focused and in front, as
//! usual (lib.rs calls this in debug builds only). In: the main window. Out: the window, minimized in the taskbar.

/// ShowWindow's command: show the window minimized, without activating it.
const SW_SHOWMINNOACTIVE: i32 = 7;

#[link(name = "user32")]
unsafe extern "system" {
    /// Shows, hides, minimizes or restores a window.
    fn ShowWindow(hwnd: isize, command: i32) -> i32;
}

/// Shows the window minimized in the taskbar, without activating it.
pub fn show_minimized(window: &tauri::WebviewWindow) {
    let Ok(hwnd) = window.hwnd() else { return };
    // SAFETY: the window's own handle, alive while `window` is; ShowWindow only changes how it is shown
    unsafe {
        ShowWindow(hwnd.0 as isize, SW_SHOWMINNOACTIVE);
    }
}
