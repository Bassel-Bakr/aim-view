//! The API without a window or a server: requests answered by `api::handle` on a library in a data folder, each answer
//! printed as a line of JSON ({"status": ..., "body": ...}), to check the answers against python/retired/server.py's
//! on copies of its data.
//! cargo run -p aimview-service --example api -- <data folder> <models folder> <requests file> [--layout app|python]
//!   [--vods <folder>] [--stats <KovaaK's stats folder>]
//! The app's layout (the default) reads the VODs folder from the data folder's settings.json; Python's layout takes
//! test_out/ as the data folder and the VODs folder from --vods. Each line of the requests file: METHOD PATH (with its
//! query), then a tab and the body when there is one; or POLL PATH KEYS: the GET asked again (for up to 10 minutes)
//! until one of its answer's KEYS (a|b) is not null; or SLEEP SECONDS: a wait, for work the library does in the
//! background (the area finder learning).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use aimview_service::{ApiRequest, Config, Layout, Library, api};
use serde_json::{Value, json};

/// The data folder's place on the command line (after the program's own name, 0).
const DATA_ARG: usize = 1;
/// The models folder's place.
const MODELS_ARG: usize = 2;
/// The requests file's place.
const REQUESTS_ARG: usize = 3;
/// How long a POLL asks again.
const POLL_LIMIT: Duration = Duration::from_secs(600);
/// How long a POLL waits between two asks.
const POLL_WAIT: Duration = Duration::from_millis(250);
/// A SLEEP's seconds when they do not read as a number.
const DEFAULT_SLEEP_S: f64 = 1.0;

/// One request answered: its status and its body as JSON (as a JSON string when it is not JSON).
fn ask(library: &Arc<Library>, method: &str, path: &str, body: &str) -> (u16, Value) {
    let request = ApiRequest { method, path_and_query: path, range: None, body: body.as_bytes(), upload: None };
    let response = api::handle(library, &request);
    let as_text = |_| json!(String::from_utf8_lossy(&response.body));
    let body = serde_json::from_slice(&response.body).unwrap_or_else(as_text);
    (response.status, body)
}

/// Asks for `path` until one of the answer's `keys` (a|b) is not null, or for `POLL_LIMIT`.
fn poll(library: &Arc<Library>, path: &str, keys: &str) -> (u16, Value) {
    let started = Instant::now();
    loop {
        let (status, answer) = ask(library, "GET", path, "");
        if keys.split('|').any(|key| !answer[key].is_null()) || started.elapsed() > POLL_LIMIT {
            break (status, answer);
        }
        std::thread::sleep(POLL_WAIT);
    }
}

/// Opens the library and answers the requests file's lines in order; a line it cannot read is skipped.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let option = |name: &str| args.iter().position(|arg| arg == name).and_then(|i| args.get(i + 1)).cloned();
    let layout = match option("--layout").as_deref() {
        None | Some("app") => Layout::App,
        Some("python") => Layout::Python,
        Some(other) => panic!("no layout called {other} (app or python)"),
    };
    let mut config = Config::new(PathBuf::from(&args[DATA_ARG]), layout, PathBuf::from(&args[MODELS_ARG]));
    config.vods = option("--vods").map(PathBuf::from);
    if let Some(stats) = option("--stats") {
        config.stats = stats.into();
    }
    let library = Library::open(config).expect("the library");
    for line in std::fs::read_to_string(&args[REQUESTS_ARG]).expect("the requests file").lines() {
        let (head, body) = line.split_once('\t').unwrap_or((line, ""));
        let parts: Vec<&str> = head.split(' ').collect();
        let (status, answer) = match parts.as_slice() {
            ["POLL", path, keys] => poll(&library, path, keys),
            ["SLEEP", seconds] => {
                std::thread::sleep(Duration::from_secs_f64(seconds.parse().unwrap_or(DEFAULT_SLEEP_S)));
                continue;
            }
            [method, path] => ask(&library, method, path, body),
            _ => continue,
        };
        println!("{}", json!({ "status": status, "body": answer }));
    }
}
