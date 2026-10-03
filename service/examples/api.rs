//! The API without a window or a server: requests answered by `api::handle` on a library in a data folder, each answer
//! printed as a line of JSON ({"status": ..., "body": ...}), to check the answers against python/server.py's on copies
//! of its data.
//! cargo run -p aimview-service --example api -- <data folder> <models folder> <requests file> [--layout app|python]
//!   [--vods <folder>] [--stats <KovaaK's stats folder>]
//! The app's layout (the default) reads the VODs folder from the data folder's settings.json; python/server.py's takes
//! test_out/ as the data folder and the VODs folder from --vods. Each line of the requests file: METHOD PATH (with its
//! query), then a tab and the body when there is one; or POLL PATH KEYS: the GET asked again (for up to 10 minutes)
//! until one of its answer's KEYS (a|b) is not null; or SLEEP SECONDS: a wait, for work the library does in the
//! background (the area finder learning).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use aimview_service::{ApiRequest, Config, Layout, Library, api};
use serde_json::{Value, json};

fn ask(lib: &Arc<Library>, method: &str, path: &str, body: &str) -> (u16, Value) {
    let res = api::handle(lib, &ApiRequest { method, path_and_query: path, range: None, body: body.as_bytes() });
    let body = serde_json::from_slice(&res.body).unwrap_or_else(|_| json!(String::from_utf8_lossy(&res.body)));
    (res.status, body)
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let option = |name: &str| a.iter().position(|v| v == name).and_then(|i| a.get(i + 1)).cloned();
    let layout = match option("--layout").as_deref() {
        None | Some("app") => Layout::App,
        Some("python") => Layout::Python,
        Some(other) => panic!("no layout called {other} (app or python)"),
    };
    let mut config = Config::new(PathBuf::from(&a[1]), layout, PathBuf::from(&a[2]));
    config.vods = option("--vods").map(PathBuf::from);
    if let Some(stats) = option("--stats") {
        config.stats = stats.into();
    }
    let lib = Library::open(config).expect("the library");
    for line in std::fs::read_to_string(&a[3]).expect("the requests file").lines() {
        let (head, body) = line.split_once('\t').unwrap_or((line, ""));
        let parts: Vec<&str> = head.split(' ').collect();
        let (status, answer) = match parts.as_slice() {
            ["POLL", path, key] => {
                let started = Instant::now();
                loop {
                    let (status, answer) = ask(&lib, "GET", path, "");
                    if key.split('|').any(|k| !answer[k].is_null()) || started.elapsed() > Duration::from_secs(600) {
                        break (status, answer);
                    }
                    std::thread::sleep(Duration::from_millis(250));
                }
            }
            ["SLEEP", seconds] => {
                std::thread::sleep(Duration::from_secs_f64(seconds.parse().unwrap_or(1.0)));
                continue;
            }
            [method, path] => ask(&lib, method, path, body),
            _ => continue,
        };
        println!("{}", json!({ "status": status, "body": answer }));
    }
}
