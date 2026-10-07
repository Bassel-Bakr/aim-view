//! The review server's API inside the app, over a custom protocol (`api`, at http://api.localhost in the window): no
//! network port, so nothing outside the app reaches it. The window's server-mode services send /api/... and /video
//! there (ui/src/app/modes/tauri/). The service answers (aimview_service::api); the routes that need the desktop are
//! answered here: the folder dialog (/api/folder) and the mouse logger's switch (/api/mouse/logger).
//!
//! In: the window's requests (lib.rs hands each one over on a thread of its own). Out: the responses, with the headers
//! that let the window's page read them.

use std::sync::Arc;

use aimview_service::{Answer, ApiRequest, ApiResponse, Library, api};
use serde_json::{Value, json};
use tauri::http::{Method, Request, Response, StatusCode};

use crate::mouse;

/// A response to the window (another origin than the page): readable there under its cross-origin isolation.
fn respond(answer: ApiResponse) -> Response<Vec<u8>> {
    let mut response = Response::builder()
        .status(StatusCode::from_u16(answer.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR))
        .header("Access-Control-Allow-Origin", "*")
        .header("Access-Control-Allow-Headers", "*")
        .header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
        .header("Access-Control-Expose-Headers", "Content-Range, Content-Length, Accept-Ranges")
        .header("Cross-Origin-Resource-Policy", "cross-origin");
    for (name, value) in &answer.headers {
        response = response.header(name.as_str(), value.as_str());
    }
    response.body(answer.body).unwrap_or_default()
}

/// A desktop route's answer as the service would give it: its JSON, or the failure's status and message.
fn json_response(answer: Answer<Value>) -> ApiResponse {
    let (status, body) = match answer {
        Ok(value) => (StatusCode::OK.as_u16(), value),
        Err(failure) => (failure.status, json!({ "error": failure.message })),
    };
    let headers = vec![("Content-Type".to_string(), "application/json".to_string())];
    ApiResponse { status, headers, body: serde_json::to_vec(&body).unwrap_or_default() }
}

/// Answers one request.
pub fn handle(lib: &Arc<Library>, req: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    if req.method() == Method::OPTIONS {
        let headers = vec![("Content-Type".to_string(), "text/plain".to_string())];
        return respond(ApiResponse { status: StatusCode::NO_CONTENT.as_u16(), headers, body: Vec::new() });
    }
    // the path and query only (the window gives the whole URL, http://api.localhost/...)
    let at = req.uri().path_and_query().map_or("/", |path_and_query| path_and_query.as_str());
    let url = tauri::Url::parse(&format!("http://api.localhost{at}")).ok();
    let path = url.as_ref().map(|url| url.path().to_string()).unwrap_or_default();
    let on = url.as_ref().is_some_and(|url| url.query_pairs().any(|(key, value)| key == "on" && value == "1"));
    let desktop = match (req.method() == Method::POST, path.as_str()) {
        (true, "/api/folder") => Some(pick_folder(lib)),
        (false, "/api/mouse/logger") => Some(Ok(mouse::logger_state())),
        (true, "/api/mouse/logger") => Some(mouse::set_logger(on)),
        _ => None,
    };
    respond(match desktop {
        Some(answer) => json_response(answer),
        None => api::handle(lib, &ApiRequest {
            method: req.method().as_str(),
            path_and_query: at,
            range: req.headers().get("Range").and_then(|range| range.to_str().ok()),
            body: req.body(),
            // the window's request comes whole, its body already in memory: an upload is written from it
            upload: None,
        }),
    })
}

/// The VODs folder, chosen in the system's folder dialog; null when the user cancels.
fn pick_folder(lib: &Library) -> Answer<Value> {
    match rfd::FileDialog::new().set_title("The folder OBS records into (one folder per scenario)").pick_folder() {
        Some(folder) => lib.set_vods(folder),
        None => Ok(Value::Null),
    }
}

/// The protocol's answers.
#[cfg(test)]
mod tests {
    use super::*;
    use aimview_service::{Config, Layout};

    /// The answer to a request with `method` and `path` (with its query) and no body.
    fn ask(lib: &Arc<Library>, method: &str, path: &str) -> Response<Vec<u8>> {
        let uri = format!("http://api.localhost{path}");
        let req = Request::builder().method(method).uri(uri).body(Vec::new()).unwrap();
        handle(lib, &req)
    }

    /// A preflight, the service's routes and the mouse logger's state all answer with the window's CORS headers; a
    /// missing video is 404.
    #[test]
    fn the_window_gets_the_services_answers() {
        let data = std::env::temp_dir().join(format!("aimview-protocol-{}", std::process::id()));
        let lib = Library::open(Config::new(data.clone(), Layout::App, data.join("models"))).unwrap();
        let cors = |response: &Response<Vec<u8>>| {
            response.headers().get("Access-Control-Allow-Origin").is_some_and(|value| value == "*")
        };
        let options = ask(&lib, "OPTIONS", "/api/vods");
        assert_eq!(options.status(), StatusCode::NO_CONTENT);
        assert!(cors(&options));
        let info = ask(&lib, "GET", "/api/info");
        assert_eq!(info.status(), StatusCode::OK);
        assert!(cors(&info) && info.headers().get("Content-Type").is_some_and(|value| value == "application/json"));
        let info: Value = serde_json::from_slice(info.body()).unwrap();
        assert_eq!(info, json!({ "detector": "full_v3", "device": aimview_service::Device::Auto.name() }));
        let video = ask(&lib, "GET", "/video?id=nope%2Fnope.mp4");
        assert_eq!(video.status(), StatusCode::NOT_FOUND);
        let state = ask(&lib, "GET", "/api/mouse/logger");
        assert_eq!(state.status(), StatusCode::OK);
        let state: Value = serde_json::from_slice(state.body()).unwrap();
        assert_eq!(state["on"], false);
        std::fs::remove_dir(&data).unwrap();
    }
}
