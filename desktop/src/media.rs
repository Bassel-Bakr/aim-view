//! How the window's player decodes videos on Linux: WebKitGTK plays them through GStreamer, which picks the decoder of
//! the highest rank. The recordings are mostly 2560 x 1440 AV1 at 120 fps, which libaom (Ubuntu's only AV1 decoder in
//! software) decodes slower than real time, so the GPU's decoders go first (VA-API for AMD and Intel, which Ubuntu
//! 22.04's GStreamer leaves unranked; NVIDIA's comes next as it is), then dav1d (the AppImage carries it), then the rest.
//! Inside the AppImage its own libva looks for the system's drivers where Ubuntu keeps them, so it is told where other
//! distributions do. Settings the user made stay.
//!
//! In: the environment at start (main.rs, before any thread). Out: GStreamer's and libva's settings, which the window's
//! web process inherits.

use std::path::Path;

/// GStreamer's setting of decoders' ranks, as "name:rank" pairs.
const RANKS_SETTING: &str = "GST_PLUGIN_FEATURE_RANK";
/// The ranks: VA-API's decoders above NVIDIA's (PRIMARY + 1, 257), dav1d below them (PRIMARY, 256), libaom below all
/// (SECONDARY, 128, as it is).
const DECODER_RANKS: &str = "vaav1dec:258,vah264dec:258,vah265dec:258,vavp9dec:258,dav1ddec:256";
/// libva's setting of where its drivers are.
const VA_DRIVERS_SETTING: &str = "LIBVA_DRIVERS_PATH";
/// Where distributions keep VA-API's drivers: Debian and Ubuntu, Fedora and openSUSE, Arch.
const VA_DRIVER_FOLDERS: [&str; 3] = ["/usr/lib/x86_64-linux-gnu/dri", "/usr/lib64/dri", "/usr/lib/dri"];
/// What an AppImage's runtime sets to the AppImage's own path.
const APPIMAGE_SETTING: &str = "APPIMAGE";

/// Sets the decoders' order and, inside the AppImage, libva's driver folders, unless the user set them. Call it first in
/// `main`, before any other thread starts: changing the environment while another thread reads it is undefined
/// behavior.
pub fn prefer_fast_decoders() {
    if std::env::var_os(RANKS_SETTING).is_none() {
        // SAFETY: called before any other thread exists (see above)
        unsafe { std::env::set_var(RANKS_SETTING, DECODER_RANKS) };
    }
    if std::env::var_os(APPIMAGE_SETTING).is_some() && std::env::var_os(VA_DRIVERS_SETTING).is_none() {
        let folders: Vec<&str> = VA_DRIVER_FOLDERS.into_iter().filter(|folder| Path::new(folder).is_dir()).collect();
        if !folders.is_empty() {
            // SAFETY: as above
            unsafe { std::env::set_var(VA_DRIVERS_SETTING, folders.join(":")) };
        }
    }
}
