//! The recordings' videos over HTTP on this computer only (127.0.0.1, at a port the system picks), for the window on
//! Linux: WebKitGTK plays no video from an app's own protocol (error 4, where `fetch` works), so the page asks for
//! /video here instead (ui/src/app/modes/tauri/desktop-recordings.ts, through `window.aimviewVideoBase`). Every
//! address starts with a token made at each start, so no other page on this computer can guess one. A request gets all
//! the bytes it asks for, to the file's end for an open range: WebKit's player takes a shorter answer (the service's
//! 4 MB parts) as the whole file, and never reaches an index at its end (OBS writes it last).
//!
//! In: the window's video requests (GET or HEAD, with or without a Range header) and the recordings' videos (the
//! library resolves an id to its file). Out: the bytes, streamed from the file, with the headers that let the window's
//! page play them.

use std::collections::hash_map::RandomState;
use std::fs::File;
use std::hash::BuildHasher;
use std::io::{self, BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use aimview_service::Library;

/// The most bytes a request's line and headers may take.
const MAX_HEAD_BYTES: usize = 16 * 1024;
/// How long a connection may wait for its request before it is dropped.
const READ_TIMEOUT: Duration = Duration::from_secs(30);
/// The route the page asks for a video at, after the token.
const VIDEO_ROUTE: &str = "/video";

/// The server's socket and token, bound before the window opens (its address goes into the page) and answering once
/// the library is open (`serve`).
pub struct VideoServer {
    /// The socket, at 127.0.0.1 and a port the system picked.
    listener: TcpListener,
    /// The random first part of every address: 32 hex digits.
    token: String,
}

impl VideoServer {
    /// A socket at 127.0.0.1 on a free port, and a new token.
    pub fn bind() -> io::Result<VideoServer> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        Ok(VideoServer { listener, token: new_token() })
    }

    /// Where the page asks for videos: "http://127.0.0.1:<port>/<token>", to which it adds "/video?id=...".
    pub fn base(&self) -> io::Result<String> {
        Ok(format!("http://{}/{}", self.listener.local_addr()?, self.token))
    }

    /// Answers on a thread of its own from now on, each connection on a thread of its own (the player opens a few).
    pub fn serve(self, lib: Arc<Library>) {
        std::thread::spawn(move || {
            for stream in self.listener.incoming().flatten() {
                let (lib, token) = (lib.clone(), self.token.clone());
                std::thread::spawn(move || {
                    if let Err(error) = answer(&lib, &token, stream) {
                        eprintln!("video server: {error}");
                    }
                });
            }
        });
    }
}

/// A token no other process can guess: two hashers' keys, which the standard library draws from the system's random
/// source, 128 bits in all.
fn new_token() -> String {
    let half = || RandomState::new().hash_one(std::process::id());
    format!("{:016x}{:016x}", half(), half())
}

/// One request on `stream`: a video's bytes when the path is the token's /video with a recording's id, else 404; the
/// connection closes after.
fn answer(lib: &Library, token: &str, stream: TcpStream) -> io::Result<()> {
    stream.set_read_timeout(Some(READ_TIMEOUT))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let (method, target, range) = read_head(&mut reader)?;
    let head_only = method == "HEAD";
    let route = target.strip_prefix('/').and_then(|rest| rest.strip_prefix(token));
    let id = route.filter(|_| method == "GET" || head_only).and_then(video_id);
    match id.map(|id| lib.resolve(&id)) {
        Some(Ok(path)) => send_video(stream, &path, range.as_deref(), head_only),
        _ => send_head(&stream, "404 Not Found", &[("Content-Type", "text/plain".into())], 0),
    }
}

/// The recording's id a path (after the token) asks for: /video?id=...; None for any other path.
fn video_id(route: &str) -> Option<String> {
    let url = tauri::Url::parse(&format!("http://video.localhost{route}")).ok()?;
    if url.path() != VIDEO_ROUTE {
        return None;
    }
    url.query_pairs().find(|(key, _)| key == "id").map(|(_, id)| id.into_owned())
}

/// A request's method, target and Range header, read up to the blank line after its headers (MAX_HEAD_BYTES at most).
fn read_head(reader: &mut impl BufRead) -> io::Result<(String, String, Option<String>)> {
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let (method, target) = (parts.next().unwrap_or_default().to_string(), parts.next().unwrap_or_default().to_string());
    let (mut range, mut read) = (None, line.len());
    loop {
        let mut header = String::new();
        let count = reader.read_line(&mut header)?;
        read += count;
        if count == 0 || header.trim().is_empty() {
            break;
        }
        if read > MAX_HEAD_BYTES {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "a request's headers too long"));
        }
        if let Some((name, value)) = header.split_once(':')
            && name.trim().eq_ignore_ascii_case("range")
        {
            range = Some(value.trim().to_string());
        }
    }
    Ok((method, target, range))
}

/// The first and last byte a Range header asks for in a file of `size` bytes ("bytes=a-b", "bytes=a-", "bytes=-n";
/// the first of several ranges), the last cut to the file's end; None when it asks for nothing in the file.
fn byte_range(range: &str, size: u64) -> Option<(u64, u64)> {
    let first = range.trim().strip_prefix("bytes=")?.split(',').next()?.trim();
    let (from, to) = first.split_once('-')?;
    let last = size.checked_sub(1)?;
    let (start, end) = match (from.trim(), to.trim()) {
        ("", "") => return None,
        ("", count) => (size.saturating_sub(count.parse().ok()?), last),
        (from, "") => (from.parse().ok()?, last),
        (from, to) => (from.parse().ok()?, to.parse::<u64>().ok()?.min(last)),
    };
    (start <= end).then_some((start, end))
}

/// The video's type for the Content-Type header, from its extension.
fn video_type(path: &Path) -> &'static str {
    match path.extension().map(|extension| extension.to_string_lossy().to_lowercase()).as_deref() {
        Some("webm") => "video/webm",
        Some("mkv") => "video/x-matroska",
        Some("mov") => "video/quicktime",
        _ => "video/mp4",
    }
}

/// The video's bytes: all of them (200) or the range asked for (206), streamed from the file; 416 for a range past its
/// end; only the headers for HEAD.
fn send_video(stream: TcpStream, path: &Path, range: Option<&str>, head_only: bool) -> io::Result<()> {
    let mut file = File::open(path)?;
    let size = file.metadata()?.len();
    let mut headers = vec![("Content-Type", video_type(path).to_string()), ("Accept-Ranges", "bytes".to_string())];
    let (status, start, length) = match range.map(|range| byte_range(range, size)) {
        None => ("200 OK", 0, size),
        Some(Some((start, end))) => {
            headers.push(("Content-Range", format!("bytes {start}-{end}/{size}")));
            ("206 Partial Content", start, end - start + 1)
        }
        Some(None) => {
            headers.push(("Content-Range", format!("bytes */{size}")));
            return send_head(&stream, "416 Range Not Satisfiable", &headers, 0);
        }
    };
    send_head(&stream, status, &headers, length)?;
    if head_only {
        return Ok(());
    }
    file.seek(SeekFrom::Start(start))?;
    let mut out = BufWriter::new(stream);
    io::copy(&mut file.take(length), &mut out)?;
    out.flush()
}

/// An answer's status line and headers, with the ones that let the window's page play it from another origin under its
/// cross-origin isolation, for a body of `length` bytes; the connection closes after it.
fn send_head(mut stream: &TcpStream, status: &str, headers: &[(&str, String)], length: u64) -> io::Result<()> {
    let mut head = format!("HTTP/1.1 {status}\r\n");
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("Access-Control-Allow-Origin: *\r\nCross-Origin-Resource-Policy: cross-origin\r\n");
    head.push_str("Access-Control-Expose-Headers: Content-Range, Content-Length, Accept-Ranges\r\n");
    head.push_str(&format!("Content-Length: {length}\r\nConnection: close\r\n\r\n"));
    stream.write_all(head.as_bytes())
}

/// The server's answers.
#[cfg(test)]
mod tests {
    use super::*;
    use aimview_service::{Config, Layout};
    use std::io::Read;

    /// The answer to `request` sent to `address`: its status line and headers as text, and its body.
    fn ask(address: &str, request: &str) -> (String, Vec<u8>) {
        let mut stream = TcpStream::connect(address).unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        let mut answer = Vec::new();
        stream.read_to_end(&mut answer).unwrap();
        let head_end = answer.windows(4).position(|window| window == b"\r\n\r\n").unwrap() + 4;
        (String::from_utf8_lossy(&answer[..head_end]).into_owned(), answer[head_end..].to_vec())
    }

    /// A video's range comes back as 206 with Content-Range and the window's headers, an open range to the file's end
    /// (not cut to a part), no range as the whole file (200), a range past the end as 416; HEAD has no body; a wrong
    /// token, another route or a missing video is 404.
    #[test]
    fn the_token_s_video_route_answers_ranges() {
        let data = std::env::temp_dir().join(format!("aimview-video-server-{}", std::process::id()));
        let lib = Library::open(Config::new(data.clone(), Layout::App, data.join("models"))).unwrap();
        let video = (0..=255u8).cycle().take(10_000).collect::<Vec<u8>>();
        let saved = lib.upload("run.mp4", None, &video).unwrap();
        assert_eq!(saved["id"], "uploads/run.mp4");
        let server = VideoServer::bind().unwrap();
        let base = server.base().unwrap();
        let (address, token) = base.strip_prefix("http://").unwrap().split_once('/').unwrap();
        let (address, token) = (address.to_string(), token.to_string());
        assert_eq!(token.len(), 32);
        server.serve(lib.clone());
        let with_range = |path: &str, method: &str, range: &str| {
            ask(&address, &format!("{method} /{path} HTTP/1.1\r\nHost: x\r\n{range}\r\n"))
        };
        let get = |path: &str, method: &str| with_range(path, method, "Range: bytes=100-199\r\n");
        let run = format!("{token}/video?id=uploads%2Frun.mp4");
        let (head, body) = get(&run, "GET");
        assert!(head.starts_with("HTTP/1.1 206 Partial Content\r\n"), "{head}");
        assert!(head.contains("Content-Range: bytes 100-199/10000\r\n"));
        assert!(head.contains("Cross-Origin-Resource-Policy: cross-origin\r\n"));
        assert_eq!(body, &video[100..200]);
        let (head, body) = with_range(&run, "GET", "Range: bytes=1000-\r\n");
        assert!(head.contains("Content-Range: bytes 1000-9999/10000\r\n"), "{head}");
        assert_eq!(body, &video[1000..]);
        let (head, body) = with_range(&run, "GET", "Range: bytes=-10\r\n");
        assert!(head.contains("Content-Range: bytes 9990-9999/10000\r\n") && body == video[9990..]);
        let (head, body) = with_range(&run, "GET", "");
        assert!(head.starts_with("HTTP/1.1 200 OK\r\n") && body == video, "{head}");
        let (head, body) = with_range(&run, "GET", "Range: bytes=10000-\r\n");
        assert!(head.starts_with("HTTP/1.1 416") && head.contains("Content-Range: bytes */10000") && body.is_empty());
        let (head, body) = get(&run, "HEAD");
        assert!(head.starts_with("HTTP/1.1 206") && head.contains("Content-Length: 100\r\n") && body.is_empty());
        let wrong_token = format!("{}{}", &token[1..], "0");
        for path in [format!("{wrong_token}/video?id=uploads%2Frun.mp4"), format!("{token}/api/vods")] {
            assert!(get(&path, "GET").0.starts_with("HTTP/1.1 404"), "{path}");
        }
        assert!(get(&format!("{token}/video?id=uploads%2Fnone.mp4"), "GET").0.starts_with("HTTP/1.1 404"));
        drop(lib);
        let _ = std::fs::remove_dir_all(&data);
    }
}
