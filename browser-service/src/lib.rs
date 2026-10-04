//! The review service (aimview-service, without its `native` feature) as WebAssembly: browser mode runs it in a worker,
//! so the browser answers the same API as the review server and the desktop app. Its files are the page's mounted
//! folders, read and written through the host's imports (service/src/disk.rs: module "host", `host_fs` the one
//! asynchronous call, which Asyncify suspends: scripts/ui-assets.ts runs wasm-opt on this module).
//!
//! Plain exports over the module's memory, as the core's (src/wasm.rs, whose `alloc` and `dealloc` this module
//! exports too): the page reserves memory with `alloc`, fills it, calls a function with pointers, and frees the result
//! block it gets back with `dealloc(ptr, its whole length)`. Every block is 8-byte aligned, as `alloc`'s.
//!
//! - `service_open(config_ptr, config_len)`: opens the library. The config is JSON: {"data": "/data", "vods": null or
//!   "/vods", "stats": "/kovaak/stats", "scenarios": ["/kovaak/scenarios", "/kovaak/workshop"], "models": "/models"}.
//!   Answers [u32 code][u32 len][len bytes] (little-endian): code 0 opened, 1 not (the bytes say why, UTF-8).
//! - `service_handle(req_ptr, req_len, body_ptr, body_len)`: answers one API request. The request is JSON:
//!   {"method": "GET" or "POST", "path": "/api/...?..."}, and for /api/upload "upload": a file the page wrote in the
//!   data folder, which is moved into place (then the body is empty); the body its bytes (may be none). Answers
//!   [u32 status][u32 type_len][type, UTF-8][u32 body_len][body] (little-endian), the whole block 12 + type_len +
//!   body_len bytes.
//!
//! One call at a time: a call can wait on the page (`host_fs`), and the library's locks are not for two calls at once.

#![cfg(target_arch = "wasm32")]

use std::alloc::{Layout, alloc};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use aimview_service::{ApiRequest, Config, Device, Ffmpeg, Layout as DataLayout, Library};
use serde::Deserialize;

/// The library `service_open` opened.
static LIBRARY: Mutex<Option<Arc<Library>>> = Mutex::new(None);

/// `service_open`'s config: the page's mounted folders.
#[derive(Deserialize)]
struct Open {
    data: PathBuf,
    #[serde(default)]
    vods: Option<PathBuf>,
    stats: PathBuf,
    #[serde(default)]
    scenarios: Vec<PathBuf>,
    models: PathBuf,
}

/// `service_handle`'s request; for /api/upload the body can be a file the page wrote in the data folder instead
/// (`upload`, `ApiRequest::upload`: moved into place, never read into memory).
#[derive(Deserialize)]
struct Request {
    method: String,
    path: String,
    #[serde(default)]
    upload: Option<PathBuf>,
}

/// A block of the parts one after the other, reserved as `alloc` reserves (8-byte aligned); the page frees it.
fn block(parts: &[&[u8]]) -> *mut u8 {
    let len: usize = parts.iter().map(|p| p.len()).sum();
    // SAFETY: the layout has a size of at least 1; each part is copied into its own range of the block
    unsafe {
        let ptr = alloc(Layout::from_size_align(len.max(1), 8).expect("a block's layout"));
        let mut at = 0;
        for p in parts {
            std::ptr::copy_nonoverlapping(p.as_ptr(), ptr.add(at), p.len());
            at += p.len();
        }
        ptr
    }
}

/// The bytes at `ptr` (none when `len` is 0).
///
/// # Safety
/// `ptr` must point to `len` bytes when `len` is not 0.
unsafe fn bytes<'a>(ptr: *const u8, len: usize) -> &'a [u8] {
    if len == 0 { &[] } else { unsafe { std::slice::from_raw_parts(ptr, len) } }
}

fn u32le(n: usize) -> [u8; 4] {
    (n as u32).to_le_bytes()
}

/// Opens the library (see the module's notes); a library opened before is replaced.
///
/// # Safety
/// `config_ptr` must point to `config_len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn service_open(config_ptr: *const u8, config_len: usize) -> *mut u8 {
    let opened = serde_json::from_slice::<Open>(unsafe { bytes(config_ptr, config_len) })
        .map_err(|e| format!("the config: {e}"))
        .and_then(|o| {
            let mut config = Config::new(o.data, DataLayout::App, o.models);
            (config.vods, config.stats, config.scenarios) = (o.vods, o.stats, o.scenarios);
            (config.device, config.ffmpeg) = (Device::Auto, Ffmpeg::Path);
            Library::open(config)
        });
    match opened {
        Ok(lib) => {
            *LIBRARY.lock().unwrap_or_else(|e| e.into_inner()) = Some(lib);
            block(&[&u32le(0), &u32le(0)])
        }
        Err(e) => block(&[&u32le(1), &u32le(e.len()), e.as_bytes()]),
    }
}

/// Answers one API request (see the module's notes).
///
/// # Safety
/// `req_ptr` must point to `req_len` bytes and `body_ptr` to `body_len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn service_handle(req_ptr: *const u8, req_len: usize, body_ptr: *const u8, body_len: usize) -> *mut u8 {
    let answer = |status: usize, kind: &str, body: &[u8]| {
        block(&[&u32le(status), &u32le(kind.len()), kind.as_bytes(), &u32le(body.len()), body])
    };
    let failed = |status: usize, message: &str| {
        let body = serde_json::to_vec(&serde_json::json!({ "error": message })).unwrap_or_default();
        answer(status, "application/json", &body)
    };
    let request = match serde_json::from_slice::<Request>(unsafe { bytes(req_ptr, req_len) }) {
        Ok(r) => r,
        Err(e) => return failed(400, &format!("the request: {e}")),
    };
    let Some(lib) = LIBRARY.lock().unwrap_or_else(|e| e.into_inner()).clone() else {
        return failed(503, "the service is not open");
    };
    let body = unsafe { bytes(body_ptr, body_len) };
    let req = ApiRequest { method: &request.method, path_and_query: &request.path, range: None, body, upload: request.upload.as_deref() };
    let r = aimview_service::handle(&lib, &req);
    let kind = r.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("Content-Type")).map_or("application/json", |(_, v)| v.as_str());
    answer(r.status.into(), kind, &r.body)
}
