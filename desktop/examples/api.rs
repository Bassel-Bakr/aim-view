//! The app's API without its window: requests answered by the library in a data folder as the window's are (api.rs),
//! each answer printed as a line of JSON ({"status": ..., "body": ...}), to check the answers against
//! python/server.py's on copies of its data.
//! cargo run -p aimview-desktop --example api -- <data folder> <models folder> <requests file>
//! Each line of the requests file: METHOD PATH (with its query), then a tab and the body when there is one; or
//! POLL PATH KEYS: the GET asked again (for up to 10 minutes) until one of its answer's KEYS (a|b) is not null; or
//! SLEEP SECONDS: a wait, for work the app does in the background (the area finder learning).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use aimview_desktop::{api, library::Library};
use serde_json::{Value, json};

fn ask(lib: &Arc<Library>, method: &str, path: &str, body: &str) -> (u16, Value) {
    let req = tauri::http::Request::builder()
        .method(method)
        .uri(format!("http://api.localhost{path}"))
        .body(body.as_bytes().to_vec())
        .expect("a request");
    let res = api::handle(lib, &req);
    let body = serde_json::from_slice(res.body()).unwrap_or_else(|_| json!(String::from_utf8_lossy(res.body())));
    (res.status().as_u16(), body)
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let lib = Arc::new(Library::new(PathBuf::from(&a[1]), PathBuf::from(&a[2])));
    if let Err(e) = lib.fix_examples() {
        eprintln!("the area finder's examples: {}", e.message);
    }
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
