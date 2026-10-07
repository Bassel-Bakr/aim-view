//! The review server's API (python/retired/server.py's), free of any web framework: a request's method, path and
//! query, Range header and body in (`ApiRequest`), the status, headers and body out (`ApiResponse`). The desktop app
//! answers its window with it (over a custom protocol), and the HTTP server answers the browser. Routes that need the
//! desktop (the folder dialog, /api/folder; the mouse logger's switch, /api/mouse/logger) are answered by the desktop
//! app before it asks here: here they are not found (404). Each route asks the library (library/) for its answer.
//!
//! The browser build (no `native` feature) answers the same routes, but for these: the page runs the review
//! (/api/analyse answers what to review; /api/job and /api/reviewed take its progress and its end), the area finder
//! (/api/find_areas answers 409 until /api/found takes what it found) and the cut-off's labels, and downloads links
//! (501); it chooses the VODs folder (/api/folder), adds raw mouse logs (/api/mouse_log) and says when it copied new
//! KovaaK files (/api/kovaak); it plays the videos itself (/video is not served).

#[cfg(feature = "native")]
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;

use serde_json::{Value, json};

use crate::library::{Answer, Failure, Library};

/// The most of a video one ranged response holds: the player asks again for the rest.
#[cfg(feature = "native")]
const VIDEO_CHUNK: u64 = 4 << 20;
/// An answer's status: done.
const OK: u16 = 200;
/// An answer's status: part of a video (every video answer is one, even without a Range header).
#[cfg(feature = "native")]
const PARTIAL_CONTENT: u16 = 206;
/// An answer's status: a route the browser build leaves to the page.
#[cfg(not(feature = "native"))]
const NOT_IMPLEMENTED: u16 = 501;
/// The Content-Type of a JSON answer.
const JSON: &str = "application/json";

/// A request: its method ("GET", "POST"), its path with its query ("/api/report?id=..."), its Range header if any, and
/// its body (in memory, or for an upload a file).
pub struct ApiRequest<'a> {
    /// "GET" or "POST" (any case); anything but POST is answered as a GET.
    pub method: &'a str,
    /// The path with its query, as "/api/report?id=...".
    pub path_and_query: &'a str,
    /// The Range header, for /video.
    pub range: Option<&'a str>,
    /// The body's bytes (empty for a GET).
    pub body: &'a [u8],
    /// An upload's body as a file instead, written to disk as it arrived (the HTTP server streams /api/upload's body
    /// into `Library::spool`'s file): /api/upload moves it into place. None: the body is `body`.
    pub upload: Option<&'a Path>,
}

/// A response: its status, its headers (Content-Type always; for a video Accept-Ranges and Content-Range) and its body.
pub struct ApiResponse {
    /// The HTTP status.
    pub status: u16,
    /// The headers, as (name, value).
    pub headers: Vec<(String, String)>,
    /// The body's bytes.
    pub body: Vec<u8>,
}

impl ApiResponse {
    /// A response with only its Content-Type header (`kind`).
    fn new(status: u16, kind: &str, body: Vec<u8>) -> ApiResponse {
        ApiResponse { status, headers: vec![("Content-Type".into(), kind.into())], body }
    }
}

/// A JSON answer: the value, or {error} with the failure's status.
fn json_response(answer: Answer<Value>) -> ApiResponse {
    match answer {
        Ok(value) => ApiResponse::new(OK, JSON, serde_json::to_vec(&value).unwrap_or_default()),
        Err(failure) => {
            let body = serde_json::to_vec(&json!({ "error": failure.message })).unwrap_or_default();
            ApiResponse::new(failure.status, JSON, body)
        }
    }
}

/// A request as the routes read it: whether it is a POST, its path, its query's values and its body.
struct Route<'a> {
    /// The request itself, for its body, Range header and upload.
    request: &'a ApiRequest<'a>,
    /// The path and query parsed as a URL; None when they cannot be.
    url: Option<url::Url>,
    /// The path without its query ("/api/report"); empty when it cannot be parsed.
    path: String,
    /// Whether the method is POST.
    post: bool,
}

impl<'a> Route<'a> {
    /// The request's path and query, parsed.
    fn new(request: &'a ApiRequest<'a>) -> Route<'a> {
        let url = url::Url::parse(&format!("http://api.localhost{}", request.path_and_query)).ok();
        let path = url.as_ref().map(|url| url.path().to_string()).unwrap_or_default();
        Route { request, url, path, post: request.method.eq_ignore_ascii_case("POST") }
    }

    /// A value of the query, by its key.
    fn query(&self, key: &str) -> Option<String> {
        let url = self.url.as_ref()?;
        url.query_pairs().find(|(name, _)| name == key).map(|(_, value)| value.into_owned())
    }

    /// Whether the query says `key=1`.
    fn flag(&self, key: &str) -> bool {
        self.query(key).as_deref() == Some("1")
    }

    /// The recording the request is about (id=).
    fn id(&self) -> Answer<String> {
        self.query("id").ok_or_else(|| Failure::bad("id= is missing"))
    }

    /// The body as JSON; null when it is not JSON.
    fn body(&self) -> Value {
        serde_json::from_slice::<Value>(self.request.body).unwrap_or(Value::Null)
    }

    /// Whether the area finder may copy areas from a recording of the same layout (copy=, 1 when it is not given).
    fn copy_areas(&self) -> bool {
        self.query("copy").as_deref().unwrap_or("1") == "1"
    }
}

/// Answers one request.
pub fn handle(library: &Arc<Library>, request: &ApiRequest) -> ApiResponse {
    let route = Route::new(request);
    if let Some(response) = special_answer(library, &route) {
        return response;
    }
    let answer = if route.post { post_answer(library, &route) } else { get_answer(library, &route) };
    json_response(answer)
}

/// The routes whose answers are not JSON values (a video, the examples' text, the tracks' bytes), or not only that
/// (the browser build's 409 for the area finder); None for the others.
fn special_answer(library: &Library, route: &Route) -> Option<ApiResponse> {
    #[cfg(feature = "native")]
    if route.path == "/video" {
        return Some(match route.id().and_then(|id| library.resolve(&id)) {
            Ok(path) => video(&path, route.request.range),
            Err(failure) => json_response(Err(failure)),
        });
    }
    if !route.post && route.path == "/api/area_kinds_file" {
        return Some(match library.kinds_file() {
            Ok(bytes) => ApiResponse::new(OK, "application/json", bytes),
            Err(failure) => json_response(Err(failure)),
        });
    }
    if !route.post && route.path == "/api/area_examples" {
        return Some(match library.examples_text() {
            Ok(text) => ApiResponse::new(OK, "text/plain; charset=utf-8", text),
            Err(failure) => json_response(Err(failure)),
        });
    }
    #[cfg(not(feature = "native"))]
    if !route.post && route.path == "/api/find_areas" {
        return Some(page_find_areas(library, route));
    }
    if !route.post && route.path == "/api/crop_image" {
        return Some(match crop_ids(route).and_then(|(page, id)| library.crop_image(&page, &id)) {
            Ok(png) => ApiResponse::new(OK, "image/png", png),
            Err(failure) => json_response(Err(failure)),
        });
    }
    if route.path == "/api/tracks" {
        return Some(match route.id().map(|id| library.tracks(&id)) {
            Ok(Some(bytes)) => ApiResponse::new(OK, JSON, bytes),
            Ok(None) => json_response(Ok(Value::Null)),
            Err(failure) => json_response(Err(failure)),
        });
    }
    None
}

/// The browser build's /api/find_areas: until the page's area finder sent what it found, it is asked to (409, `need`,
/// with the video to read).
#[cfg(not(feature = "native"))]
fn page_find_areas(library: &Library, route: &Route) -> ApiResponse {
    match route.id().and_then(|id| library.find_areas(&id, route.copy_areas())) {
        Err(failure) if failure.status == crate::library::FOUND_NEEDED => {
            let video = route.id().and_then(|id| library.resolve(&id)).ok();
            let body = json!({ "error": failure.message, "need": "found", "video": video });
            ApiResponse::new(failure.status, JSON, serde_json::to_vec(&body).unwrap_or_default())
        }
        answer => json_response(answer),
    }
}

/// The answer to a GET request.
fn get_answer(library: &Library, route: &Route) -> Answer<Value> {
    let id = || route.id();
    match route.path.as_str() {
        "/api/vods" => library.recordings(route.flag("quick")),
        "/api/models" => library.models(),
        "/api/job" => id().map(|id| library.job(&id)),
        "/api/report" => id().and_then(|id| library.report(&id)),
        "/api/run" => id().and_then(|id| library.marks(&id)),
        "/api/stats" => id().and_then(|id| library.stats_info(&id, route.query("q").as_deref())),
        "/api/history" => {
            let scenario = route.query("scenario").ok_or_else(|| Failure::bad("scenario= is missing"));
            scenario.and_then(|scenario| library.history(&scenario))
        }
        "/api/mouse" => id().and_then(|id| library.mouse_measures(&id)),
        "/api/info" => Ok(json!({ "detector": library.model(), "device": library.config().device.name() })),
        "/api/exclude" => {
            let kovobs = route.query("layout").as_deref() == Some("kovobs");
            library.exclude_answer(route.query("id").as_deref(), kovobs)
        }
        "/api/find_areas" => id().and_then(|id| library.find_areas(&id, route.copy_areas())),
        "/api/label_queue" => library.label_queue(),
        "/api/faint" => id().map(|id| library.faint(&id)),
        "/api/faint_queue" => library.faint_queue(),
        "/api/crop_pages" => library.crop_pages(),
        "/api/crops" => crop_set(route).and_then(|(page, set)| library.crops(&page, &set)),
        "/api/crop_answers" => crop_set(route).and_then(|(page, set)| library.crop_answers(&page, &set)),
        "/api/crop_export" => page(route).and_then(|page| library.export_crop_answers(&page)),
        path => Err(Failure::missing(format!("not found: {path}"))),
    }
}

/// The answer to a POST request.
fn post_answer(library: &Arc<Library>, route: &Route) -> Answer<Value> {
    let id = || route.id();
    let body = route.request.body;
    match route.path.as_str() {
        "/api/model" => library.pick(&route.query("name").unwrap_or_default()),
        "/api/device" => library.use_device(&route.query("name").unwrap_or_default()),
        "/api/batch" => library.use_batch(&route.query("n").unwrap_or_default()),
        "/api/analyse" => id().and_then(|id| library.analyse(&id, route.flag("again"))),
        "/api/cancel" => id().and_then(|id| library.cancel(&id)),
        "/api/run" => id().and_then(|id| library.set_marks(&id, &route.body())),
        "/api/stats" => id().and_then(|id| library.set_stats(&id, &route.body())),
        "/api/upload" => upload(library, route),
        #[cfg(feature = "native")]
        "/api/link/formats" => library.link_formats(&route.body()),
        #[cfg(feature = "native")]
        "/api/link" => library.add_link(&route.body()),
        #[cfg(not(feature = "native"))]
        "/api/link/formats" | "/api/link" => {
            Err(Failure { status: NOT_IMPLEMENTED, message: "the page downloads links itself".into() })
        }
        "/api/exclude" => id().and_then(|id| library.set_exclude(&id, body)),
        "/api/area_kinds" => library.save_kind(&route.body()),
        "/api/label_skip" => id().and_then(|id| library.skip_label(&id)),
        "/api/not_aim" => {
            let on = route.query("on").as_deref().unwrap_or("1") == "1";
            id().and_then(|id| library.set_not_aim(&id, on))
        }
        "/api/faint" => id().and_then(|id| library.set_faint(&id, &route.body(), None)),
        "/api/faint_skip" => id().and_then(|id| library.skip_faint(&id)),
        "/api/faint_submit" => id().and_then(|id| library.submit_faint(&id, offset(route.query("offset"))?)),
        "/api/area_examples" => library.set_examples(body),
        "/api/area_kinds_file" => library.set_kinds_file(body),
        "/api/crop_answer" => crop_ids(route).and_then(|(page, id)| library.save_crop_answer(&page, &id, body)),
        "/api/crop_import" => page(route).and_then(|page| library.import_crop_answers(&page, body)),
        #[cfg(not(feature = "native"))]
        "/api/folder" => library.choose_vods(&route.query("path").unwrap_or_default()),
        #[cfg(not(feature = "native"))]
        "/api/job" => id().and_then(|id| library.page_progress(&id, body)),
        #[cfg(not(feature = "native"))]
        "/api/reviewed" => id().and_then(|id| library.review_done(&id, body)),
        #[cfg(not(feature = "native"))]
        "/api/found" => id().and_then(|id| library.keep_found(&id, body)),
        #[cfg(not(feature = "native"))]
        "/api/mouse_log" => library.keep_mouse_log(&route.query("name").unwrap_or_default(), body),
        #[cfg(not(feature = "native"))]
        "/api/kovaak" if route.flag("changed") => library.kovaak_changed(),
        path => Err(Failure::missing(format!("not found: {path}"))),
    }
}

/// /api/upload: a file added (name=), for a recording (id=, a stats file) or as a recording of its own.
fn upload(library: &Library, route: &Route) -> Answer<Value> {
    let (name, recording) = (route.query("name").unwrap_or_default(), route.query("id"));
    match route.request.upload {
        Some(file) => library.upload_file(&name, recording.as_deref(), file),
        None => library.upload(&name, recording.as_deref(), route.request.body),
    }
}

/// The check folder a Crops request is about (page=).
fn page(route: &Route) -> Answer<String> {
    route.query("page").ok_or_else(|| Failure::bad("page= is missing"))
}

/// A check folder and one of its sets (page=, set=).
fn crop_set(route: &Route) -> Answer<(String, String)> {
    Ok((page(route)?, route.query("set").ok_or_else(|| Failure::bad("set= is missing"))?))
}

/// A check folder and one of its crops (page=, id=).
fn crop_ids(route: &Route) -> Answer<(String, String)> {
    Ok((page(route)?, route.id()?))
}

/// A cut-off's offset from the query (python/retired/server.py: `float(q.get("offset", 0.3))`); an error, in Python's
/// words, when it is not a number.
fn offset(text: Option<String>) -> Answer<f64> {
    text.map_or(Ok(aimview::faint::DEFAULT_OFFSET), |text| {
        text.trim().parse().map_err(|_| Failure::bad(format!("could not convert string to float: '{text}'")))
    })
}

/// The bytes a Range header asks for (bytes=from-to, bytes=from- or bytes=-last) of a file of `size` bytes, as
/// (start, end), both in: all of it without one. An end past the file ends at its last byte.
#[cfg(feature = "native")]
fn asked_range(range: Option<&str>, size: u64) -> (u64, u64) {
    let last = size.saturating_sub(1);
    let asked = range.and_then(|range| range.strip_prefix("bytes=")).and_then(|range| range.split_once('-'));
    match asked {
        Some((from, to)) if !from.is_empty() => (from.parse().unwrap_or(0), to.parse().unwrap_or(last).min(last)),
        Some((_, to)) if !to.is_empty() => (size.saturating_sub(to.parse().unwrap_or(0)), last),
        _ => (0, last),
    }
}

/// A video, or the part of it a Range header asks for (at most VIDEO_CHUNK bytes), so the player can seek.
#[cfg(feature = "native")]
fn video(path: &Path, range: Option<&str>) -> ApiResponse {
    let Ok(mut file) = std::fs::File::open(path) else {
        return json_response(Err(Failure::missing("the video is gone")));
    };
    let size = file.metadata().map_or(0, |metadata| metadata.len());
    let (start, end) = asked_range(range, size);
    let end = end.min(start + VIDEO_CHUNK - 1);
    let mut body = vec![0u8; (end + 1).saturating_sub(start) as usize];
    if file.seek(SeekFrom::Start(start)).and_then(|_| file.read_exact(&mut body)).is_err() {
        return json_response(Err("the video could not be read".to_string().into()));
    }
    let kind = match path.extension().map(|extension| extension.to_string_lossy().to_lowercase()).as_deref() {
        Some("webm") => "video/webm",
        Some("mkv") => "video/x-matroska",
        Some("mov") => "video/quicktime",
        _ => "video/mp4",
    };
    let mut response = ApiResponse::new(PARTIAL_CONTENT, kind, body);
    response.headers.push(("Accept-Ranges".into(), "bytes".into()));
    response.headers.push(("Content-Range".into(), format!("bytes {start}-{end}/{size}")));
    response
}
