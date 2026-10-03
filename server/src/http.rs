//! The HTTP side: each request is checked (access.rs), then goes to the review API, to the old page (/old/) or to the
//! UI's files. The API answers on a blocking thread of its own: a listing or a report reads files, and a review's start
//! can take a while (the review itself runs in the background, polled with /api/job).

use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

use axum::Router;
use axum::body::{Body, Bytes, HttpBody};
use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::header::{
    ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_ALLOW_METHODS, ACCESS_CONTROL_ALLOW_ORIGIN, ACCESS_CONTROL_EXPOSE_HEADERS,
    ACCESS_CONTROL_MAX_AGE, CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE, LOCATION, RANGE, SET_COOKIE, VARY,
};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio_util::io::ReaderStream;

use crate::access::{Access, Verdict, loopback_caller};
use crate::files::{self, Found};

/// One request to the review API.
pub struct Call {
    pub method: String,
    pub path_and_query: String,
    pub range: Option<String>,
    pub body: Bytes,
    /// An upload's body, written to this file as it arrived (`body` is then empty). The API moves it into place; what
    /// it leaves is removed after it answers.
    pub upload: Option<PathBuf>,
}

/// The review API's answer. `headers` hold its Content-Type.
pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// The review API (aimview_service::api, or a stand-in in the tests).
pub trait Api: Send + Sync + 'static {
    fn handle(&self, call: &Call) -> Reply;
    /// A new file for an upload's body (POST /api/upload), on the disk the API keeps the uploads on.
    fn spool(&self) -> Result<PathBuf, String>;
}

pub struct App {
    pub api: Arc<dyn Api>,
    pub access: Access,
    /// The UI's build (index.html and its files)
    pub ui: PathBuf,
    /// The old page (python/app/), at /old/
    pub old: PathBuf,
}

/// Every path goes through `answer`; uploads (a whole video) have no size limit: they go to disk as they arrive.
pub fn router(app: Arc<App>) -> Router {
    Router::new().fallback(answer).with_state(app).layer(DefaultBodyLimit::disable())
}

/// A plain-text answer.
fn text(status: StatusCode, body: impl Into<String>) -> Response {
    (status, [(CONTENT_TYPE, "text/plain; charset=utf-8")], body.into()).into_response()
}

/// Answers a request, and logs it unless it is one of the many a page makes while it works: the UI's files, video
/// ranges and review progress (/api/job). Failures are always logged.
async fn answer(State(app): State<Arc<App>>, req: Request) -> Response {
    let started = Instant::now();
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let response = route(&app, req).await;
    let status = response.status();
    let routine = !files::is_api(&path) || path == "/api/job" || path == "/video";
    if !(routine && (status.is_success() || status.is_redirection())) {
        println!("{method} {path} {} {} ms", status.as_u16(), started.elapsed().as_millis());
    }
    response
}

async fn route(app: &App, req: Request) -> Response {
    match app.access.check(req.method(), req.uri(), req.headers()) {
        Verdict::Pass => {}
        Verdict::SetCookie { location, cookie } => {
            return (StatusCode::SEE_OTHER, [(LOCATION, location), (SET_COOKIE, cookie)]).into_response();
        }
        Verdict::Refuse { status, reason } => return text(status, format!("Aim View: {reason}\n")),
    }
    if files::is_api(req.uri().path()) {
        // a page on this machine other than the server's (the UI in browser mode) may read the answers
        let caller = loopback_caller(req.headers());
        if req.method() == Method::OPTIONS {
            let mut r = StatusCode::NO_CONTENT.into_response();
            if let Some(origin) = caller {
                let h = r.headers_mut();
                h.insert(ACCESS_CONTROL_ALLOW_METHODS, HeaderValue::from_static("GET, POST"));
                h.insert(ACCESS_CONTROL_ALLOW_HEADERS, HeaderValue::from_static("content-type, range"));
                h.insert(ACCESS_CONTROL_MAX_AGE, HeaderValue::from_static("600"));
                allow(h, origin);
            }
            return r;
        }
        let mut r = call_api(app, req).await;
        if let Some(origin) = caller {
            allow(r.headers_mut(), origin);
        }
        return r;
    }
    if !matches!(*req.method(), Method::GET | Method::HEAD) {
        return text(StatusCode::METHOD_NOT_ALLOWED, "the UI's files are read with GET\n");
    }
    let path = req.uri().path();
    if path == "/old" {
        // the old page names its files relative to /old/
        return (StatusCode::MOVED_PERMANENTLY, [(LOCATION, "/old/")]).into_response();
    }
    if let Some(rest) = path.strip_prefix("/old/") {
        return old_file(app, rest).await;
    }
    ui_file(app, path).await
}

async fn call_api(app: &App, req: Request) -> Response {
    let (parts, body) = req.into_parts();
    // an upload (a whole video, gigabytes) is written to a file as it arrives; the other bodies are small
    let (body, upload) = if parts.method == Method::POST && parts.uri.path() == "/api/upload" {
        let file = match app.api.spool() {
            Ok(f) => f,
            Err(e) => return text(StatusCode::INTERNAL_SERVER_ERROR, format!("the upload has nowhere to go: {e}\n")),
        };
        if let Err(failed) = spool(body, &file).await {
            let _ = tokio::fs::remove_file(&file).await;
            return failed;
        }
        (Bytes::new(), Some(file))
    } else {
        match axum::body::to_bytes(body, usize::MAX).await {
            Ok(b) => (b, None),
            Err(_) => return text(StatusCode::BAD_REQUEST, "the request's body could not be read\n"),
        }
    };
    let call = Call {
        // HEAD is GET without the body, which the HTTP library leaves out
        method: if parts.method == Method::HEAD { "GET".into() } else { parts.method.as_str().into() },
        path_and_query: parts.uri.path_and_query().map_or("/", |p| p.as_str()).into(),
        range: parts.headers.get(RANGE).and_then(|r| r.to_str().ok()).map(String::from),
        body,
        upload: upload.clone(),
    };
    let api = app.api.clone();
    let answered = tokio::task::spawn_blocking(move || api.handle(&call)).await;
    if let Some(file) = upload {
        // the API moved it into place, or did not take it
        let _ = tokio::fs::remove_file(&file).await;
    }
    let Ok(reply) = answered else {
        return text(StatusCode::INTERNAL_SERVER_ERROR, "the review service failed on this request\n");
    };
    let mut response = Response::new(Body::from(reply.body));
    *response.status_mut() = StatusCode::from_u16(reply.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let headers = response.headers_mut();
    for (name, value) in reply.headers {
        // the UI is on the server's own origin: no other site's page may read the answers
        if name.to_ascii_lowercase().starts_with("access-control-") || name.eq_ignore_ascii_case("cross-origin-resource-policy") {
            continue;
        }
        if let (Ok(name), Ok(value)) = (HeaderName::try_from(name), HeaderValue::try_from(value)) {
            headers.append(name, value);
        }
    }
    response
}

/// Lets the page at `origin` read the answer, and the video's ranges.
fn allow(headers: &mut HeaderMap, origin: HeaderValue) {
    headers.insert(ACCESS_CONTROL_ALLOW_ORIGIN, origin);
    headers.insert(ACCESS_CONTROL_EXPOSE_HEADERS, HeaderValue::from_static("content-range"));
    headers.append(VARY, HeaderValue::from_static("origin"));
}

/// Writes a request's body to `file` as it arrives, so an upload of any size takes little memory; on failure, the
/// answer to give (the body broke off, or the file could not be written).
async fn spool(mut body: Body, file: &Path) -> Result<(), Response> {
    let unwritten = |e: std::io::Error| text(StatusCode::INTERNAL_SERVER_ERROR, format!("the upload could not be written: {e}\n"));
    let mut out = BufWriter::with_capacity(1 << 20, tokio::fs::File::create(file).await.map_err(unwritten)?);
    while let Some(frame) = std::future::poll_fn(|cx| Pin::new(&mut body).poll_frame(cx)).await {
        let frame = frame.map_err(|e| text(StatusCode::BAD_REQUEST, format!("the upload broke off: {e}\n")))?;
        if let Ok(data) = frame.into_data() {
            out.write_all(&data).await.map_err(unwritten)?;
        }
    }
    // all of it on disk, and the file closed, before the API moves it
    out.flush().await.map_err(unwritten)?;
    Ok(())
}

/// A file, streamed, with its type and length; None when it cannot be opened.
async fn file_response(file: &Path) -> Option<Response> {
    let f = tokio::fs::File::open(file).await.ok()?;
    let length = f.metadata().await.map(|m| m.len()).ok();
    let mut response = Response::new(Body::from_stream(ReaderStream::new(f)));
    let headers = response.headers_mut();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static(files::content_type(file)));
    if let Some(n) = length {
        headers.insert(CONTENT_LENGTH, n.into());
    }
    headers.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    Some(response)
}

/// One of the UI's files, or index.html for the app's own pages, with the cross-origin isolation the Angular dev
/// server gives (ui/angular.json).
async fn ui_file(app: &App, path: &str) -> Response {
    let (file, page) = match files::find(&app.ui, path) {
        Found::File(f) => (f, false),
        Found::Index => (app.ui.join("index.html"), true),
    };
    let Some(mut response) = file_response(&file).await else {
        let missing = format!("the UI is not built: {} is missing (bun run build:server)\n", file.display());
        return text(StatusCode::NOT_FOUND, if page { missing } else { "not found\n".into() });
    };
    let headers = response.headers_mut();
    headers.insert("cross-origin-opener-policy", HeaderValue::from_static("same-origin"));
    headers.insert("cross-origin-embedder-policy", HeaderValue::from_static("require-corp"));
    if page {
        // a new build's page names new files: never kept
        headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    }
    response
}

/// One of the old page's files (python/app/), as python/server.py served them: index.html for /old/, never kept, and
/// no cross-origin isolation (the page needs none). A file it does not have is not found.
async fn old_file(app: &App, rest: &str) -> Response {
    let file = match files::find(&app.old, rest) {
        Found::File(f) => f,
        Found::Index if rest.is_empty() => app.old.join("index.html"),
        Found::Index => return text(StatusCode::NOT_FOUND, "not found\n"),
    };
    let Some(mut response) = file_response(&file).await else {
        return text(StatusCode::NOT_FOUND, format!("no old page: {} is missing\n", file.display()));
    };
    response.headers_mut().insert(CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    response
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use axum::http::header::HOST;
    use tower::ServiceExt;

    use super::*;

    /// A call as the API got it: method, path and query, range, body.
    type Seen = (String, String, Option<String>, Vec<u8>);

    /// The API's stand-in: answers every call with what it was asked, and keeps the calls.
    #[derive(Default)]
    struct Echo {
        calls: Mutex<Vec<Seen>>,
    }

    impl Api for Echo {
        fn handle(&self, call: &Call) -> Reply {
            // an upload's body is read back from its file
            let body = call.upload.as_ref().map_or_else(|| call.body.to_vec(), |f| std::fs::read(f).unwrap());
            self.calls.lock().unwrap().push((call.method.clone(), call.path_and_query.clone(), call.range.clone(), body));
            Reply {
                status: if call.path_and_query.starts_with("/video") { 206 } else { 200 },
                headers: vec![
                    ("Content-Type".into(), "application/json".into()),
                    ("Access-Control-Allow-Origin".into(), "*".into()),
                    ("Content-Range".into(), "bytes 0-1/10".into()),
                ],
                body: b"{}".to_vec(),
            }
        }

        fn spool(&self) -> Result<PathBuf, String> {
            Ok(std::env::temp_dir().join(format!("aimview-server-upload-{}.part", std::process::id())))
        }
    }

    fn ui() -> PathBuf {
        let root = std::env::temp_dir().join(format!("aimview-server-http-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("index.html"), "<!doctype html><title>Aim View</title>").unwrap();
        std::fs::write(root.join("main.js"), "console.log(1)").unwrap();
        root
    }

    fn old() -> PathBuf {
        let root = std::env::temp_dir().join(format!("aimview-server-old-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("index.html"), "<!doctype html><title>Old page</title>").unwrap();
        std::fs::write(root.join("app.js"), "old()").unwrap();
        root
    }

    fn app(token: Option<&str>) -> (Arc<Echo>, Router) {
        let echo = Arc::new(Echo::default());
        let addrs = ["127.0.0.1:8770".parse().unwrap()];
        let access = Access::new(&addrs, token.map(String::from)).unwrap();
        (echo.clone(), router(Arc::new(App { api: echo, access, ui: ui(), old: old() })))
    }

    async fn send(router: &Router, req: axum::http::request::Builder, body: &'static [u8]) -> (StatusCode, Response) {
        let response = router.clone().oneshot(req.header(HOST, "127.0.0.1:8770").body(Body::from(body)).unwrap()).await.unwrap();
        (response.status(), response)
    }

    async fn body(r: Response) -> String {
        String::from_utf8(axum::body::to_bytes(r.into_body(), usize::MAX).await.unwrap().to_vec()).unwrap()
    }

    #[tokio::test]
    async fn the_apps_pages_get_index_html() {
        let (echo, router) = app(None);
        for path in ["/", "/run/Gridshot%20-%2099%20-%202026.10.02-12.00.00.mp4", "/models", "/no-such.js"] {
            let (status, r) = send(&router, Request::get(path), b"").await;
            assert_eq!(status, StatusCode::OK, "{path}");
            assert_eq!(r.headers()[CONTENT_TYPE], "text/html; charset=utf-8");
            assert_eq!(r.headers()[CACHE_CONTROL], "no-cache");
            assert_eq!(r.headers()["cross-origin-embedder-policy"], "require-corp");
            assert!(body(r).await.contains("<title>Aim View</title>"));
        }
        let (status, r) = send(&router, Request::get("/main.js"), b"").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(r.headers()[CONTENT_TYPE], "text/javascript; charset=utf-8");
        assert_eq!(body(r).await, "console.log(1)");
        assert!(echo.calls.lock().unwrap().is_empty(), "no page went to the API");
        let (status, _) = send(&router, Request::post("/"), b"").await;
        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    }

    #[tokio::test]
    async fn the_old_page_is_at_old() {
        let (echo, router) = app(None);
        let (status, r) = send(&router, Request::get("/old"), b"").await;
        assert_eq!(status, StatusCode::MOVED_PERMANENTLY);
        assert_eq!(r.headers()[LOCATION], "/old/");
        let (status, r) = send(&router, Request::get("/old/"), b"").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(r.headers()[CONTENT_TYPE], "text/html; charset=utf-8");
        assert_eq!(r.headers()[CACHE_CONTROL], "no-cache");
        assert!(r.headers().get("cross-origin-embedder-policy").is_none());
        assert!(body(r).await.contains("<title>Old page</title>"));
        let (status, r) = send(&router, Request::get("/old/app.js"), b"").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(r.headers()[CONTENT_TYPE], "text/javascript; charset=utf-8");
        assert_eq!(body(r).await, "old()");
        // a file it does not have, and never a file outside its folder
        for path in ["/old/missing.js", "/old/../Cargo.toml", "/old/%2e%2e/x", "/old//"] {
            let (status, _) = send(&router, Request::get(path), b"").await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
        }
        assert!(echo.calls.lock().unwrap().is_empty(), "nothing went to the API");
    }

    #[tokio::test]
    async fn the_api_gets_the_method_query_range_and_body() {
        let (echo, router) = app(None);
        let (status, r) = send(&router, Request::post("/api/run?id=a%20b").header(RANGE, "bytes=5-"), b"{\"start\":1}").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(r.headers()[CONTENT_TYPE], "application/json");
        assert!(r.headers().get("access-control-allow-origin").is_none(), "no other site reads the answers");
        let (status, r) = send(&router, Request::get("/video?id=x").header(RANGE, "bytes=0-1"), b"").await;
        assert_eq!(status, StatusCode::PARTIAL_CONTENT);
        assert_eq!(r.headers()["content-range"], "bytes 0-1/10");
        let (status, _) = send(&router, Request::get("/api/nothing"), b"").await;
        assert_eq!(status, StatusCode::OK, "the API answers its own unknown paths");
        // the UI in browser mode, on another port of this machine, reads the answers and the video's ranges
        let page = |r: axum::http::request::Builder| r.header("origin", "http://localhost:4200").header("sec-fetch-site", "cross-site");
        let (status, r) = send(&router, page(Request::options("/api/link")), b"").await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_eq!(r.headers()["access-control-allow-origin"], "http://localhost:4200");
        assert_eq!(r.headers()["access-control-allow-headers"], "content-type, range");
        let (status, r) = send(&router, page(Request::get("/video?id=x").header(RANGE, "bytes=0-1")), b"").await;
        assert_eq!(status, StatusCode::PARTIAL_CONTENT);
        assert_eq!(r.headers()["access-control-allow-origin"], "http://localhost:4200");
        assert_eq!(r.headers()["access-control-expose-headers"], "content-range");
        let calls = echo.calls.lock().unwrap();
        assert_eq!(calls[0], ("POST".into(), "/api/run?id=a%20b".into(), Some("bytes=5-".into()), b"{\"start\":1}".to_vec()));
        assert_eq!(calls[1].1, "/video?id=x");
        assert_eq!(calls[2].1, "/api/nothing");
        assert_eq!(calls.len(), 4, "a preflight never reaches the API");
    }

    #[tokio::test]
    async fn an_upload_reaches_the_api_as_a_file() {
        let (echo, router) = app(None);
        let video: Vec<u8> = (0..3_000_000u32).map(|i| (i % 251) as u8).collect();
        let req = Request::post("/api/upload?name=a.mp4").header(HOST, "127.0.0.1:8770").body(Body::from(video.clone())).unwrap();
        let response = router.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let calls = echo.calls.lock().unwrap();
        assert_eq!(calls[0].1, "/api/upload?name=a.mp4");
        assert!(calls[0].3 == video, "the file holds the whole body");
        assert!(!echo.spool().unwrap().exists(), "the file the API left is removed");
    }

    #[tokio::test]
    async fn a_token_guards_the_api_and_the_ui() {
        let (echo, router) = app(Some("tok"));
        let (status, _) = send(&router, Request::get("/api/vods"), b"").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (status, _) = send(&router, Request::get("/"), b"").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(echo.calls.lock().unwrap().is_empty());
        let (status, r) = send(&router, Request::get("/?token=tok"), b"").await;
        assert_eq!(status, StatusCode::SEE_OTHER);
        assert_eq!(r.headers()[LOCATION], "/");
        let cookie = r.headers()[SET_COOKIE].to_str().unwrap().split(';').next().unwrap().to_string();
        let (status, _) = send(&router, Request::get("/").header("cookie", cookie), b"").await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = send(&router, Request::get("/api/vods").header("authorization", "Bearer tok"), b"").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(echo.calls.lock().unwrap().len(), 1);
    }
}
