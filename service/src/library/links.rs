//! Recordings added from a link: a video's page on a site yt-dlp reads (YouTube, Twitch, Medal, Streamable...) or a
//! video file's address. yt-dlp (ytdlp.rs) reads the link's title and qualities; the chosen quality is downloaded in a
//! job (/api/job: stage "downloading", megabytes done of how many) into a folder of its own in the uploads, and moved
//! into place as one MP4 file when it is complete, so the list never shows half a file. Once it is there the job is
//! gone (stage "none"); a failure ends it with yt-dlp's reason.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use super::names::{local_stamp, parse_name};
use super::reviews::Job;
use super::{Answer, Failure, Library};
use crate::ytdlp::{self, LinkInfo};

/// The longest title kept in a file name, in characters.
const TITLE_CHARS: usize = 150;

/// The link in a request's body ({url}), when it is an http(s) address.
fn link_url(body: &Value) -> Answer<String> {
    let text = body["url"].as_str().unwrap_or_default().trim();
    let ok = url::Url::parse(text)
        .ok()
        .is_some_and(|u| matches!(u.scheme(), "http" | "https") && u.host_str().is_some_and(|h| !h.is_empty()));
    if !ok {
        return Err(Failure::bad("Paste a link that starts with http:// or https://"));
    }
    Ok(text.to_string())
}

/// A title as a file name: no character Windows forbids, spaces where they were, at most TITLE_CHARS long.
fn safe_title(title: &str) -> String {
    let spaced: String = title.chars().map(|c| if c.is_control() || r#"<>:"/\|?*"#.contains(c) { ' ' } else { c }).collect();
    let words = spaced.split_whitespace().collect::<Vec<_>>().join(" ");
    let short: String = words.chars().take(TITLE_CHARS).collect();
    let trimmed = short.trim_matches(|c: char| c == '.' || c == ' ');
    if trimmed.is_empty() { "video".into() } else { trimmed.into() }
}

/// When the video was uploaded, as a file-name stamp: its time, else its day, else now (seconds since 1970).
fn link_stamp(timestamp: Option<f64>, upload_date: Option<&str>, now: f64) -> String {
    let day = upload_date.filter(|d| d.len() == 8 && d.bytes().all(|b| b.is_ascii_digit()));
    match (timestamp, day) {
        (Some(t), _) => local_stamp(t),
        (None, Some(d)) => format!("{}.{}.{}-00.00.00", &d[..4], &d[4..6], &d[6..]),
        (None, None) => local_stamp(now),
    }
}

/// A link's file name: its title when it is a KovOBS recording's name ("<scenario> - <score> - <stamp>"), else
/// "<title> - <stamp>.mp4".
fn link_name(title: &str, stamp: &str) -> String {
    let title = safe_title(title);
    let kovobs = format!("{title}.mp4");
    if parse_name(&kovobs).is_some() { kovobs } else { format!("{title} - {stamp}.mp4") }
}

/// Moves a finished download into place, never over a file that is there.
fn place(file: &Path, dest: &Path) -> Result<(), String> {
    let moved = match std::fs::hard_link(file, dest) {
        Ok(()) => std::fs::remove_file(file),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(e),
        // a file system without hard links
        Err(_) if !dest.exists() => std::fs::rename(file, dest),
        Err(e) => Err(e),
    };
    moved.map_err(|e| format!("the video could not be put in the uploads: {e}"))
}

impl Library {
    /// Where yt-dlp is downloaded when the PATH has none.
    fn tools(&self) -> PathBuf {
        ytdlp::tools_folder(&self.config.ffmpeg, &self.config.data)
    }

    /// A link's title and length, and its qualities best first, from yt-dlp (fetched first when it is missing).
    fn read_link(&self, url: &str) -> Answer<LinkInfo> {
        let program = ytdlp::ensure(&self.tools(), |_, _| {})?;
        ytdlp::info(&program, url).map_err(|e| Failure::bad(format!("yt-dlp cannot read this link: {e}")))
    }

    /// A link's title, length and qualities ({url}: {title, duration, formats}), kept for the download that follows.
    pub fn link_formats(&self, body: &Value) -> Answer<Value> {
        let url = link_url(body)?;
        let info = self.read_link(&url)?;
        let answer = json!(info);
        self.links.lock().map_err(|_| "the links are broken".to_string())?.insert(url, info);
        Ok(answer)
    }

    /// Adds a recording from a link ({url, format}: format is one of `link_formats`' ids, or null for the best). It
    /// answers at once with the new recording's id and row; the download runs in a job, followed with /api/job.
    pub fn add_link(self: &Arc<Self>, body: &Value) -> Answer<Value> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let url = link_url(body)?;
        let format = body["format"].as_str().map(String::from);
        let kept = self.links.lock().map_err(|_| "the links are broken".to_string())?.remove(&url);
        let info = match kept {
            Some(info) => info,
            None => self.read_link(&url)?,
        };
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64());
        let name = link_name(&info.title, &link_stamp(info.timestamp, info.upload_date.as_deref(), now));
        std::fs::create_dir_all(self.uploads()).map_err(|e| e.to_string())?;
        let mut jobs = self.jobs.lock().map_err(|_| "the jobs are broken".to_string())?;
        // a free name: no file of it, and no other link downloading into it
        let taken = |p: &Path| {
            let id = format!("uploads/{}", p.file_name().map(|n| n.to_string_lossy()).unwrap_or_default());
            p.exists() || jobs.get(&id).is_some_and(|j| j.lock().is_ok_and(|j| j.link && j.stage != "error"))
        };
        let first = self.uploads().join(&name);
        let (stem, mut dest, mut n) = (name.trim_end_matches(".mp4").to_string(), first, 2);
        while taken(&dest) {
            dest = self.uploads().join(format!("{stem} ({n}).mp4"));
            n += 1;
        }
        let saved = dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let id = format!("uploads/{saved}");
        let job = Arc::new(Mutex::new(Job::download()));
        jobs.insert(id.clone(), job.clone());
        drop(jobs);
        let folder = self.uploads().join(format!(".link-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let spec = ytdlp::format_spec(format.as_deref(), &info.formats);
        let (lib, at, started) = (self.clone(), id.clone(), Instant::now());
        std::thread::spawn(move || {
            let progress = |stage: &str, done: usize, total: usize| {
                if let Ok(mut j) = job.lock() {
                    (j.stage, j.done, j.total) = (stage.into(), done, total);
                }
            };
            let outcome = ytdlp::ensure(&lib.tools(), |mb, of| progress("yt-dlp", mb, of))
                .and_then(|program| crate::ffmpeg::ensure(|mb, of| progress("ffmpeg", mb, of)).map(|()| program))
                .and_then(|program| {
                    progress("downloading", 0, 1);
                    std::fs::create_dir_all(&folder).map_err(|e| format!("the download's folder could not be made: {e}"))?;
                    let ffmpeg = crate::ffmpeg::program("ffmpeg");
                    let ffmpeg = ffmpeg.parent().is_some_and(|p| !p.as_os_str().is_empty()).then_some(ffmpeg.as_path());
                    let file = ytdlp::download(&program, &url, &spec, &folder, ffmpeg, |d, t| progress("downloading", d, t))?;
                    place(&file, &dest)
                });
            let _ = std::fs::remove_dir_all(&folder);
            let seconds = (started.elapsed().as_secs_f64() * 10.0).round() / 10.0;
            match outcome {
                Ok(()) => {
                    println!("downloaded {at}: {seconds} s");
                    if let Ok(mut jobs) = lib.jobs.lock()
                        && jobs.get(&at).is_some_and(|j| Arc::ptr_eq(j, &job))
                    {
                        jobs.remove(&at);
                    }
                }
                Err(e) => {
                    println!("the download of {at} failed: {e}");
                    if let Ok(mut j) = job.lock() {
                        (j.stage, j.error) = ("error".into(), Some(e));
                    }
                }
            }
        });
        let mut row = self.upload_row(&self.uploads().join(&saved), &self.not_aim(), false);
        row["mtime"] = json!(now);
        Ok(json!({ "id": id, "saved": saved, "title": info.title, "recording": row }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Layout};

    #[test]
    fn link_names() {
        let kovobs = "1wall 6targets small - 1931.23 - 2026.10.01-16.23.04";
        assert_eq!(link_name(kovobs, "2026.10.04-12.00.00"), format!("{kovobs}.mp4"));
        assert_eq!(
            link_name("Best flicks: 1wall | 6 targets?", "2026.10.04-12.00.00"),
            "Best flicks 1wall 6 targets - 2026.10.04-12.00.00.mp4"
        );
        assert_eq!(link_name(" ..\t", "2026.10.04-12.00.00"), "video - 2026.10.04-12.00.00.mp4");
        assert_eq!(safe_title(&"x".repeat(400)).len(), TITLE_CHARS);
        assert_eq!(link_stamp(None, Some("20261001"), 0.0), "2026.10.01-00.00.00");
        assert_eq!(link_stamp(None, Some("2026"), 1e9), local_stamp(1e9));
        assert_eq!(link_stamp(Some(1e9), Some("20261001"), 0.0), local_stamp(1e9));
    }

    #[test]
    fn only_web_links() {
        for url in ["https://www.youtube.com/watch?v=x", " http://example.com/a.mp4 "] {
            assert!(link_url(&json!({ "url": url })).is_ok(), "{url}");
        }
        for body in [json!({ "url": "ftp://example.com/a.mp4" }), json!({ "url": "file:///C:/a.mp4" }), json!({}), json!({ "url": "youtube.com" })] {
            assert_eq!(link_url(&body).unwrap_err().status, 400, "{body}");
        }
    }

    #[test]
    fn a_download_is_moved_into_place_and_never_over_a_file() {
        let dir = std::env::temp_dir().join(format!("aimview-place-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (file, dest) = (dir.join("video.mp4"), dir.join("run.mp4"));
        std::fs::write(&file, b"new").unwrap();
        std::fs::write(&dest, b"old").unwrap();
        assert!(place(&file, &dest).is_err());
        assert_eq!(std::fs::read(&dest).unwrap(), b"old");
        std::fs::remove_file(&dest).unwrap();
        place(&file, &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"new");
        assert!(!file.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A stand-in for yt-dlp: -J answers a title and one quality; a download writes video.mp4 where -o says. With
    /// `fail`, it fails as yt-dlp does on a private video.
    fn stand_in(dir: &Path, fail: bool) -> PathBuf {
        let info = r#"{"title": "Air - 1 - 2026.10.01-16.23.03", "duration": 3, "formats": [{"format_id": "18", "vcodec": "avc1.42001E", "acodec": "mp4a.40.2", "width": 640, "height": 360, "fps": 30}]}"#;
        let error = "ERROR: [youtube] abc: Private video. Sign in if you've been granted access to this video";
        #[cfg(windows)]
        let (file, text) = (
            dir.join(if fail { "fail.cmd" } else { "yt-dlp.cmd" }),
            if fail {
                format!("@echo off\r\necho {error} 1>&2\r\nexit /b 1\r\n")
            } else {
                format!(
                    "@echo off\r\nif \"%~1\"==\"-J\" (\r\necho {info}\r\nexit /b 0\r\n)\r\n:next\r\nif \"%~1\"==\"\" exit /b 1\r\n\
                     if \"%~1\"==\"-o\" (\r\necho aimview 18 5 10 NA\r\necho video> \"%~dp2video.mp4\"\r\nexit /b 0\r\n)\r\nshift\r\ngoto next\r\n"
                )
            },
        );
        #[cfg(unix)]
        let (file, text) = (
            dir.join(if fail { "fail.sh" } else { "yt-dlp.sh" }),
            if fail {
                format!("#!/bin/sh\necho \"{error}\" >&2\nexit 1\n")
            } else {
                format!(
                    "#!/bin/sh\nif [ \"$1\" = \"-J\" ]; then echo '{info}'; exit 0; fi\nwhile [ $# -gt 0 ]; do\n\
                     if [ \"$1\" = \"-o\" ]; then echo 'aimview 18 5 10 NA'; echo video > \"$(dirname \"$2\")/video.mp4\"; exit 0; fi\n\
                     shift\ndone\nexit 1\n"
                )
            },
        );
        std::fs::write(&file, text).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        file
    }

    /// The routes, with the stand-in for yt-dlp: the qualities, the new recording's id at once, the download's job,
    /// and the file in the list once it is done; a link yt-dlp cannot read fails with its reason.
    #[test]
    fn a_link_becomes_a_recording() {
        let dir = std::env::temp_dir().join(format!("aimview-links-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let lib = Library::open(Config::new(dir.clone(), Layout::App, dir.join("models"))).unwrap();
        ytdlp::set_stand_in(Some(stand_in(&dir, false)));
        let url = json!({ "url": "https://www.youtube.com/watch?v=abc" });
        let formats = lib.link_formats(&url).unwrap();
        assert_eq!(formats["title"], "Air - 1 - 2026.10.01-16.23.03");
        assert_eq!(formats["formats"][0]["id"], "18");
        let added = lib.add_link(&json!({ "url": url["url"], "format": "18" })).unwrap();
        let id = added["id"].as_str().unwrap().to_string();
        assert_eq!(id, "uploads/Air - 1 - 2026.10.01-16.23.03.mp4");
        assert_eq!(added["recording"]["scenario"], "Air");
        let started = Instant::now();
        while lib.job(&id)["stage"] != "none" {
            assert!(lib.job(&id)["stage"] != "error", "{}", lib.job(&id));
            assert!(started.elapsed().as_secs() < 20, "the download never ended");
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let list = lib.recordings(false).unwrap();
        assert!(list.as_array().unwrap().iter().any(|r| r["id"] == id.as_str()), "{list}");
        // only the video: the download's folder is gone
        let names: Vec<String> =
            std::fs::read_dir(lib.uploads()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["Air - 1 - 2026.10.01-16.23.03.mp4"]);
        // the same link again: a name of its own
        let again = lib.add_link(&json!({ "url": url["url"], "format": null })).unwrap();
        assert_eq!(again["id"], "uploads/Air - 1 - 2026.10.01-16.23.03 (2).mp4");
        while lib.job(again["id"].as_str().unwrap())["stage"] != "none" {
            assert!(started.elapsed().as_secs() < 20, "the download never ended");
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        ytdlp::set_stand_in(Some(stand_in(&dir, true)));
        let failed = lib.link_formats(&json!({ "url": "https://www.youtube.com/watch?v=private" })).unwrap_err();
        assert_eq!(failed.status, 400);
        assert_eq!(failed.message, "yt-dlp cannot read this link: Private video. Sign in if you've been granted access to this video");
        ytdlp::set_stand_in(None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
