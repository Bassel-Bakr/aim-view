//! The review server's API (python/server.py) inside the app, over a custom protocol (`api`, at http://api.localhost
//! in the window): no network port, so nothing outside the app reaches it. The window's server-mode services send
//! /api/... and /video there (ui/src/app/modes/tauri/).

use std::io::{Read, Seek, SeekFrom};
use std::sync::Arc;

use serde_json::{Value, json};
use tauri::http::{Method, Request, Response, StatusCode};

use crate::library::{Answer, Failure, Library};

/// The most of a video one ranged response holds: the player asks again for the rest.
const VIDEO_CHUNK: u64 = 4 << 20;

/// A response to the window (another origin than the page): readable there under its cross-origin isolation.
fn respond(status: StatusCode, kind: &str, body: Vec<u8>) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header("Content-Type", kind)
        .header("Access-Control-Allow-Origin", "*")
        .header("Access-Control-Allow-Headers", "*")
        .header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
        .header("Access-Control-Expose-Headers", "Content-Range, Content-Length, Accept-Ranges")
        .header("Cross-Origin-Resource-Policy", "cross-origin")
        .body(body)
        .unwrap_or_default()
}

fn json_response(answer: Answer<Value>) -> Response<Vec<u8>> {
    match answer {
        Ok(v) => respond(StatusCode::OK, "application/json", serde_json::to_vec(&v).unwrap_or_default()),
        Err(f) => respond(
            StatusCode::from_u16(f.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            "application/json",
            serde_json::to_vec(&json!({ "error": f.message })).unwrap_or_default(),
        ),
    }
}

/// Answers one request.
pub fn handle(lib: &Arc<Library>, req: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    if req.method() == Method::OPTIONS {
        return respond(StatusCode::NO_CONTENT, "text/plain", Vec::new());
    }
    // the path and query only (the window gives the whole URL, http://api.localhost/...)
    let at = req.uri().path_and_query().map_or("/", |p| p.as_str());
    let url = tauri::Url::parse(&format!("http://api.localhost{at}")).ok();
    let path = url.as_ref().map(|u| u.path().to_string()).unwrap_or_default();
    let query = |key: &str| url.as_ref().and_then(|u| u.query_pairs().find(|(k, _)| k == key).map(|(_, v)| v.into_owned()));
    let id = || query("id").ok_or_else(|| Failure::bad("id= is missing"));
    let body = || serde_json::from_slice::<Value>(req.body()).unwrap_or(Value::Null);
    let post = req.method() == Method::POST;
    if path == "/video" {
        return match id().and_then(|id| lib.resolve(&id)) {
            Ok(p) => video(&p, req.headers().get("Range").and_then(|r| r.to_str().ok())),
            Err(f) => json_response(Err(f)),
        };
    }
    if path == "/api/tracks" {
        return match id().map(|id| lib.tracks(&id)) {
            Ok(Some(bytes)) => respond(StatusCode::OK, "application/json", bytes),
            Ok(None) => json_response(Ok(Value::Null)),
            Err(f) => json_response(Err(f)),
        };
    }
    json_response(match (post, path.as_str()) {
        (false, "/api/vods") => lib.recordings(),
        (false, "/api/models") => lib.models(),
        (true, "/api/model") => lib.pick(&query("name").unwrap_or_default()),
        (false, "/api/job") => id().map(|id| lib.job(&id)),
        (true, "/api/analyse") => id().and_then(|id| lib.analyse(&id, query("again").as_deref() == Some("1"))),
        (false, "/api/report") => id().and_then(|id| lib.report(&id)),
        (false, "/api/stats") => id().and_then(|id| lib.stats_info(&id, query("q").as_deref())),
        (true, "/api/stats") => id().and_then(|id| lib.set_stats(&id, &body())),
        (true, "/api/upload") => lib.upload(&query("name").unwrap_or_default(), query("id").as_deref(), req.body()),
        (true, "/api/folder") => pick_folder(lib),
        _ => Err(Failure::missing(format!("not found: {path}"))),
    })
}

/// The VODs folder, chosen in the system's folder dialog; null when the user cancels.
fn pick_folder(lib: &Library) -> Answer<Value> {
    match rfd::FileDialog::new().set_title("The folder OBS records into (one folder per scenario)").pick_folder() {
        Some(folder) => lib.set_vods(folder),
        None => Ok(Value::Null),
    }
}

/// A video, or the part of it a Range header asks for (at most VIDEO_CHUNK bytes), so the player can seek.
fn video(p: &std::path::Path, range: Option<&str>) -> Response<Vec<u8>> {
    let Ok(mut f) = std::fs::File::open(p) else {
        return json_response(Err(Failure::missing("the video is gone")));
    };
    let size = f.metadata().map_or(0, |m| m.len());
    let asked = range.and_then(|r| r.strip_prefix("bytes=")).and_then(|r| r.split_once('-'));
    let (start, end) = match asked {
        Some((a, b)) if !a.is_empty() => {
            let start = a.parse().unwrap_or(0);
            (start, b.parse().unwrap_or(size.saturating_sub(1)).min(size.saturating_sub(1)))
        }
        Some((_, b)) if !b.is_empty() => (size.saturating_sub(b.parse().unwrap_or(0)), size.saturating_sub(1)),
        _ => (0, size.saturating_sub(1)),
    };
    let end = end.min(start + VIDEO_CHUNK - 1);
    let mut body = vec![0u8; (end + 1).saturating_sub(start) as usize];
    if f.seek(SeekFrom::Start(start)).and_then(|_| f.read_exact(&mut body)).is_err() {
        return json_response(Err("the video could not be read".to_string().into()));
    }
    let kind = match p.extension().map(|e| e.to_string_lossy().to_lowercase()).as_deref() {
        Some("webm") => "video/webm",
        Some("mkv") => "video/x-matroska",
        Some("mov") => "video/quicktime",
        _ => "video/mp4",
    };
    let mut r = respond(StatusCode::PARTIAL_CONTENT, kind, body);
    let h = r.headers_mut();
    h.insert("Accept-Ranges", "bytes".parse().expect("a header value"));
    if let Ok(v) = format!("bytes {start}-{end}/{size}").parse() {
        h.insert("Content-Range", v);
    }
    r
}
