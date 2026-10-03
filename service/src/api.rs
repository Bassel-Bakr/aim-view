//! The review server's API (python/server.py), free of any web framework: a request's method, path and query, Range
//! header and body in (`ApiRequest`), the status, headers and body out (`ApiResponse`). The desktop app answers its
//! window with it (over a custom protocol), and the HTTP server answers the browser. Routes that need the desktop (the
//! folder dialog, /api/folder; the mouse logger's switch, /api/mouse/logger) are answered by the desktop app before it
//! asks here: here they are not found (404).

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;

use serde_json::{Value, json};

use crate::library::{Answer, Failure, Library};

/// The most of a video one ranged response holds: the player asks again for the rest.
const VIDEO_CHUNK: u64 = 4 << 20;

/// A request: its method ("GET", "POST"), its path with its query ("/api/report?id=..."), its Range header if any, and
/// its body (in memory, or for an upload a file).
pub struct ApiRequest<'a> {
    pub method: &'a str,
    pub path_and_query: &'a str,
    pub range: Option<&'a str>,
    pub body: &'a [u8],
    /// An upload's body as a file instead, written to disk as it arrived (the HTTP server streams /api/upload's body
    /// into `Library::spool`'s file): /api/upload moves it into place. None: the body is `body`.
    pub upload: Option<&'a Path>,
}

/// A response: its status, its headers (Content-Type always; for a video Accept-Ranges and Content-Range) and its body.
pub struct ApiResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl ApiResponse {
    fn new(status: u16, kind: &str, body: Vec<u8>) -> ApiResponse {
        ApiResponse { status, headers: vec![("Content-Type".into(), kind.into())], body }
    }
}

fn json_response(answer: Answer<Value>) -> ApiResponse {
    match answer {
        Ok(v) => ApiResponse::new(200, "application/json", serde_json::to_vec(&v).unwrap_or_default()),
        Err(f) => ApiResponse::new(f.status, "application/json", serde_json::to_vec(&json!({ "error": f.message })).unwrap_or_default()),
    }
}

/// Answers one request.
pub fn handle(lib: &Arc<Library>, req: &ApiRequest) -> ApiResponse {
    let url = url::Url::parse(&format!("http://api.localhost{}", req.path_and_query)).ok();
    let path = url.as_ref().map(|u| u.path().to_string()).unwrap_or_default();
    let query = |key: &str| url.as_ref().and_then(|u| u.query_pairs().find(|(k, _)| k == key).map(|(_, v)| v.into_owned()));
    let id = || query("id").ok_or_else(|| Failure::bad("id= is missing"));
    let body = || serde_json::from_slice::<Value>(req.body).unwrap_or(Value::Null);
    let post = req.method.eq_ignore_ascii_case("POST");
    if path == "/video" {
        return match id().and_then(|id| lib.resolve(&id)) {
            Ok(p) => video(&p, req.range),
            Err(f) => json_response(Err(f)),
        };
    }
    if path == "/api/tracks" {
        return match id().map(|id| lib.tracks(&id)) {
            Ok(Some(bytes)) => ApiResponse::new(200, "application/json", bytes),
            Ok(None) => json_response(Ok(Value::Null)),
            Err(f) => json_response(Err(f)),
        };
    }
    json_response(match (post, path.as_str()) {
        (false, "/api/vods") => lib.recordings(),
        (false, "/api/models") => lib.models(),
        (true, "/api/model") => lib.pick(&query("name").unwrap_or_default()),
        (true, "/api/device") => lib.use_device(&query("name").unwrap_or_default()),
        (true, "/api/batch") => lib.use_batch(&query("n").unwrap_or_default()),
        (false, "/api/job") => id().map(|id| lib.job(&id)),
        (true, "/api/analyse") => id().and_then(|id| lib.analyse(&id, query("again").as_deref() == Some("1"))),
        (false, "/api/report") => id().and_then(|id| lib.report(&id)),
        (false, "/api/run") => id().and_then(|id| lib.marks(&id)),
        (true, "/api/run") => id().and_then(|id| lib.set_marks(&id, &body())),
        (false, "/api/stats") => id().and_then(|id| lib.stats_info(&id, query("q").as_deref())),
        (true, "/api/stats") => id().and_then(|id| lib.set_stats(&id, &body())),
        (false, "/api/history") => {
            query("scenario").ok_or_else(|| Failure::bad("scenario= is missing")).and_then(|s| lib.history(&s))
        }
        (true, "/api/upload") => {
            let (name, of) = (query("name").unwrap_or_default(), query("id"));
            match req.upload {
                Some(file) => lib.upload_file(&name, of.as_deref(), file),
                None => lib.upload(&name, of.as_deref(), req.body),
            }
        }
        (true, "/api/link/formats") => lib.link_formats(&body()),
        (true, "/api/link") => lib.add_link(&body()),
        (false, "/api/mouse") => id().and_then(|id| lib.mouse_measures(&id)),
        (false, "/api/info") => Ok(json!({ "detector": lib.model(), "device": lib.config().device.name() })),
        (false, "/api/exclude") => lib.exclude_answer(query("id").as_deref(), query("layout").as_deref() == Some("kovobs")),
        (true, "/api/exclude") => id().and_then(|id| lib.set_exclude(&id, req.body)),
        (false, "/api/find_areas") => id().and_then(|id| lib.find_areas(&id, query("copy").as_deref().unwrap_or("1") == "1")),
        (true, "/api/area_kinds") => lib.save_kind(&body()),
        (false, "/api/label_queue") => lib.label_queue(),
        (true, "/api/label_skip") => id().and_then(|id| lib.skip_label(&id)),
        (true, "/api/not_aim") => id().and_then(|id| lib.set_not_aim(&id, query("on").as_deref().unwrap_or("1") == "1")),
        (false, "/api/faint") => id().map(|id| lib.faint(&id)),
        (true, "/api/faint") => id().and_then(|id| lib.set_faint(&id, &body(), None)),
        (false, "/api/faint_queue") => lib.faint_queue(),
        (true, "/api/faint_skip") => id().and_then(|id| lib.skip_faint(&id)),
        (true, "/api/faint_submit") => id().and_then(|id| lib.submit_faint(&id, offset(query("offset"))?)),
        _ => Err(Failure::missing(format!("not found: {path}"))),
    })
}

/// A cut-off's offset from the query (python/server.py: `float(q.get("offset", 0.3))`).
fn offset(q: Option<String>) -> Answer<f64> {
    q.map_or(Ok(aimview::faint::DEFAULT_OFFSET), |o| {
        o.trim().parse().map_err(|_| Failure::bad(format!("could not convert string to float: '{o}'")))
    })
}

/// A video, or the part of it a Range header asks for (at most VIDEO_CHUNK bytes), so the player can seek.
fn video(p: &Path, range: Option<&str>) -> ApiResponse {
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
    let mut r = ApiResponse::new(206, kind, body);
    r.headers.push(("Accept-Ranges".into(), "bytes".into()));
    r.headers.push(("Content-Range".into(), format!("bytes {start}-{end}/{size}")));
    r
}
