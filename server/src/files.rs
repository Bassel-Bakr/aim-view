//! The UI's files (its server-mode build). A path that is not a file there is one of the single-page app's own
//! pages: it gets index.html, and the app shows that page.

use std::path::{Path, PathBuf};

/// Whether a path is the review API's (python/server.py's /api/... and /video), not the UI's.
pub fn is_api(path: &str) -> bool {
    path == "/api" || path.starts_with("/api/") || path == "/video"
}

/// What a UI path is.
#[derive(Debug, PartialEq)]
pub enum Found {
    /// A file of the build
    File(PathBuf),
    /// One of the app's pages: index.html
    Index,
}

/// The build's file for a URL path, or the app's index.html when there is none. A path that tries to leave the
/// build's folder (`..`, a drive, a backslash) is a page too, never a file outside it.
pub fn find(root: &Path, path: &str) -> Found {
    let decoded = percent_encoding::percent_decode_str(path).decode_utf8_lossy();
    let mut file = root.to_path_buf();
    let mut parts = 0;
    for part in decoded.split('/').filter(|p| !p.is_empty()) {
        if part == "." || part == ".." || part.contains(['\\', ':', '\0']) {
            return Found::Index;
        }
        file.push(part);
        parts += 1;
    }
    if parts > 0 && file.is_file() { Found::File(file) } else { Found::Index }
}

/// A file's content type, by its extension.
pub fn content_type(file: &Path) -> &'static str {
    let ext = file.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "webmanifest" => "application/manifest+json",
        "wasm" => "application/wasm",
        "txt" => "text/plain; charset=utf-8",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        // the detector's models, and anything else
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small build: index.html, a script and a model in a folder.
    fn build() -> PathBuf {
        let root = std::env::temp_dir().join(format!("aimview-server-files-{}", std::process::id()));
        std::fs::create_dir_all(root.join("models")).unwrap();
        std::fs::write(root.join("index.html"), "<!doctype html>").unwrap();
        std::fs::write(root.join("main-ABC.js"), "").unwrap();
        std::fs::write(root.join("models").join("detector small.onnx"), "").unwrap();
        root
    }

    #[test]
    fn the_api_and_the_videos_are_not_the_uis() {
        for p in ["/api", "/api/vods", "/api/job", "/video"] {
            assert!(is_api(p), "{p}");
        }
        for p in ["/", "/apiary", "/videos", "/video/1", "/run/api", "/index.html"] {
            assert!(!is_api(p), "{p}");
        }
    }

    #[test]
    fn files_of_the_build_and_the_single_page_fallback() {
        let root = build();
        assert_eq!(find(&root, "/main-ABC.js"), Found::File(root.join("main-ABC.js")));
        assert_eq!(find(&root, "/models/detector%20small.onnx"), Found::File(root.join("models").join("detector small.onnx")));
        assert_eq!(find(&root, "/index.html"), Found::File(root.join("index.html")));
        // the app's own pages, and files it does not have
        for p in ["/", "", "/run/Gridshot - 99 - 2026.10.02-12.00.00.mp4", "/recordings", "/models", "/missing.js"] {
            assert_eq!(find(&root, p), Found::Index, "{p}");
        }
        // never a file outside the build
        for p in ["/../Cargo.toml", "/models/../../x", "/%2e%2e/x", "/..%5Cx", "/C:/Windows/win.ini", "/models/.."] {
            assert_eq!(find(&root, p), Found::Index, "{p}");
        }
    }

    #[test]
    fn content_types() {
        assert_eq!(content_type(Path::new("a/index.html")), "text/html; charset=utf-8");
        assert_eq!(content_type(Path::new("main-ABC.js")), "text/javascript; charset=utf-8");
        assert_eq!(content_type(Path::new("styles.CSS")), "text/css; charset=utf-8");
        assert_eq!(content_type(Path::new("core/aimview_bg.wasm")), "application/wasm");
        assert_eq!(content_type(Path::new("models/detector_full_v3_u8in.onnx")), "application/octet-stream");
        assert_eq!(content_type(Path::new("favicon.ico")), "image/x-icon");
    }
}
