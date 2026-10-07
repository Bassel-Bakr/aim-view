//! Recordings added from a link: a video's page on a site yt-dlp reads (YouTube, Twitch, Medal, Streamable...) or a
//! video file's address. yt-dlp (ytdlp.rs) reads the link's title and qualities; the chosen quality is downloaded in a
//! job (/api/job: stage "downloading", megabytes done of how many) into a folder of its own in the uploads, and moved
//! into place as one MP4 file when it is complete, so the list never shows half a file. Once it is there the job is
//! gone (stage "none"); a failure ends it with yt-dlp's reason. In: /api/link/formats and /api/link. Out: the video in
//! the uploads, and the new recording's row.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use super::names::{local_stamp, parse_name};
use super::reviews::{Job, seconds_since};
use super::{Answer, Failure, Library};
use crate::ytdlp::{self, LinkInfo};

/// The longest title kept in a file name, in characters.
const TITLE_CHARS: usize = 150;
/// The digits of yt-dlp's upload date (YYYYMMDD).
const UPLOAD_DATE_DIGITS: usize = 8;

/// The link in a request's body ({url}), when it is an http(s) address.
fn link_url(body: &Value) -> Answer<String> {
    let text = body["url"].as_str().unwrap_or_default().trim();
    let web = |parsed: url::Url| {
        matches!(parsed.scheme(), "http" | "https") && parsed.host_str().is_some_and(|host| !host.is_empty())
    };
    if !url::Url::parse(text).ok().is_some_and(web) {
        return Err(Failure::bad("Paste a link that starts with http:// or https://"));
    }
    Ok(text.to_string())
}

/// A title as a file name: no character Windows forbids, spaces where they were, at most TITLE_CHARS long.
fn safe_title(title: &str) -> String {
    let forbidden = |character: char| character.is_control() || r#"<>:"/\|?*"#.contains(character);
    let spaced: String = title.chars().map(|character| if forbidden(character) { ' ' } else { character }).collect();
    let words = spaced.split_whitespace().collect::<Vec<_>>().join(" ");
    let short: String = words.chars().take(TITLE_CHARS).collect();
    let trimmed = short.trim_matches(|character: char| character == '.' || character == ' ');
    if trimmed.is_empty() { "video".into() } else { trimmed.into() }
}

/// When the video was uploaded, as a file-name stamp: its time, else its day, else now (seconds since 1970).
fn link_stamp(timestamp: Option<f64>, upload_date: Option<&str>, now: f64) -> String {
    let all_digits = |day: &&str| day.len() == UPLOAD_DATE_DIGITS && day.bytes().all(|byte| byte.is_ascii_digit());
    match (timestamp, upload_date.filter(all_digits)) {
        (Some(time), _) => local_stamp(time),
        (None, Some(day)) => format!("{}.{}.{}-00.00.00", &day[..4], &day[4..6], &day[6..]),
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
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Err(error),
        // a file system without hard links
        Err(_) if !dest.exists() => std::fs::rename(file, dest),
        Err(error) => Err(error),
    };
    moved.map_err(|error| format!("the video could not be put in the uploads: {error}"))
}

/// Where a link's video goes in `uploads`: `name`, or "<stem> (2).mp4" and so on while a file has that name or
/// another link downloads into it (`jobs`, the library's).
fn free_destination(uploads: &Path, name: &str, jobs: &HashMap<String, Arc<Mutex<Job>>>) -> PathBuf {
    let downloading = |job: &Arc<Mutex<Job>>| job.lock().is_ok_and(|job| job.link && job.stage != "error");
    let taken = |path: &Path| {
        let id = format!("uploads/{}", path.file_name().map(|name| name.to_string_lossy()).unwrap_or_default());
        path.exists() || jobs.get(&id).is_some_and(downloading)
    };
    let stem = name.trim_end_matches(".mp4");
    let mut dest = uploads.join(name);
    let mut copy = 2;
    while taken(&dest) {
        dest = uploads.join(format!("{stem} ({copy}).mp4"));
        copy += 1;
    }
    dest
}

/// Fetches yt-dlp (into `tools`) and ffmpeg when they are missing, downloads `url` in the format `spec` into `folder`
/// and moves the video to `dest`; `progress` hears each stage.
fn download_link(
    tools: &Path,
    url: &str,
    spec: &str,
    folder: &Path,
    dest: &Path,
    cancel: &Arc<AtomicBool>,
    progress: &dyn Fn(&str, usize, usize),
) -> Result<(), String> {
    let program = ytdlp::ensure(tools, |megabytes, of| progress("yt-dlp", megabytes, of))?;
    crate::ffmpeg::ensure(|megabytes, of| progress("ffmpeg", megabytes, of))?;
    if cancel.load(Ordering::Relaxed) {
        return Err(crate::review::CANCELLED.into());
    }
    progress("downloading", 0, 1);
    std::fs::create_dir_all(folder).map_err(|error| format!("the download's folder could not be made: {error}"))?;
    // the review's ffmpeg merges the parts; a bare name is the PATH's, which yt-dlp finds itself
    let ffmpeg = crate::ffmpeg::program("ffmpeg");
    let ffmpeg = ffmpeg.parent().is_some_and(|parent| !parent.as_os_str().is_empty()).then_some(ffmpeg.as_path());
    let file = ytdlp::download(program.as_path(), url, spec, folder, ffmpeg, cancel, |done, total| {
        progress("downloading", done, total);
    })?;
    place(&file, dest)
}

/// A link's download: the link, yt-dlp's format, the folder it downloads into, where the video goes, and the new
/// recording's id and job.
struct Download {
    url: String,
    spec: String,
    folder: PathBuf,
    dest: PathBuf,
    id: String,
    job: Arc<Mutex<Job>>,
}

impl Library {
    /// Where yt-dlp is downloaded when the PATH has none.
    fn tools(&self) -> PathBuf {
        ytdlp::tools_folder(&self.config.ffmpeg, &self.config.data)
    }

    /// The links' qualities read so far, by link.
    fn read_links(&self) -> Answer<MutexGuard<'_, HashMap<String, LinkInfo>>> {
        self.links.lock().map_err(|_| Failure::from("the links are broken".to_string()))
    }

    /// A link's title and length, and its qualities best first, from yt-dlp (fetched first when it is missing).
    fn read_link(&self, url: &str) -> Answer<LinkInfo> {
        let program = ytdlp::ensure(&self.tools(), |_, _| {})?;
        ytdlp::info(&program, url).map_err(|error| Failure::bad(format!("yt-dlp cannot read this link: {error}")))
    }

    /// A link's title, length and qualities ({url}: {title, duration, formats}), kept for the download that follows.
    pub fn link_formats(&self, body: &Value) -> Answer<Value> {
        let url = link_url(body)?;
        let info = self.read_link(&url)?;
        let answer = json!(info);
        self.read_links()?.insert(url, info);
        Ok(answer)
    }

    /// Adds a recording from a link ({url, format}: format is one of `link_formats`' ids, or null for the best). It
    /// answers at once with the new recording's id and row; the download runs in a job, followed with /api/job.
    pub fn add_link(self: &Arc<Self>, body: &Value) -> Answer<Value> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let url = link_url(body)?;
        let format = body["format"].as_str().map(String::from);
        let kept = self.read_links()?.remove(&url);
        let info = match kept {
            Some(info) => info,
            None => self.read_link(&url)?,
        };
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |since| since.as_secs_f64());
        let name = link_name(&info.title, &link_stamp(info.timestamp, info.upload_date.as_deref(), now));
        std::fs::create_dir_all(self.uploads()).map_err(|error| error.to_string())?;
        let mut jobs = self.jobs.lock().map_err(|_| "the jobs are broken".to_string())?;
        let dest = free_destination(&self.uploads(), &name, &jobs);
        let saved = dest.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let id = format!("uploads/{saved}");
        let job = Arc::new(Mutex::new(Job::download()));
        jobs.insert(id.clone(), job.clone());
        drop(jobs);
        let folder_name = format!(".link-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed));
        let spec = ytdlp::format_spec(format.as_deref(), &info.formats);
        let folder = self.uploads().join(folder_name);
        self.start_download(Download { url, spec, folder, dest, id: id.clone(), job });
        let mut row = self.upload_row(&self.uploads().join(&saved), &self.not_aim(), false);
        row["mtime"] = json!(now);
        Ok(json!({ "id": id, "saved": saved, "title": info.title, "recording": row }))
    }

    /// Runs a download in a thread of its own (`download_link`), its progress and its end kept in its job.
    fn start_download(self: &Arc<Self>, download: Download) {
        let (library, started) = (self.clone(), Instant::now());
        std::thread::spawn(move || {
            let Download { url, spec, folder, dest, id, job } = download;
            let cancel = job.lock().map(|job| job.cancel.clone()).unwrap_or_default();
            let progress = |stage: &str, done: usize, total: usize| {
                if let Ok(mut job) = job.lock()
                    && job.running()
                {
                    (job.stage, job.done, job.total) = (stage.into(), done, total);
                }
            };
            let outcome = download_link(&library.tools(), &url, &spec, &folder, &dest, &cancel, &progress);
            let _ = std::fs::remove_dir_all(&folder);
            library.end_download(&id, &job, outcome, seconds_since(started));
        });
    }

    /// A download's end: its job gone once the video is in place (the list shows it), or ended with the error.
    fn end_download(&self, id: &str, job: &Arc<Mutex<Job>>, outcome: Result<(), String>, seconds: f64) {
        match outcome {
            Ok(()) => {
                println!("downloaded {id}: {seconds} s");
                if let Ok(mut jobs) = self.jobs.lock()
                    && jobs.get(id).is_some_and(|kept| Arc::ptr_eq(kept, job))
                {
                    jobs.remove(id);
                }
            }
            Err(_) if job.lock().is_ok_and(|job| job.cancelled()) => {
                println!("the download of {id} was cancelled");
            }
            Err(error) => {
                println!("the download of {id} failed: {error}");
                if let Ok(mut job) = job.lock() {
                    (job.stage, job.error) = ("error".into(), Some(error));
                }
            }
        }
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
        let refused = [
            json!({ "url": "ftp://example.com/a.mp4" }),
            json!({ "url": "file:///C:/a.mp4" }),
            json!({}),
            json!({ "url": "youtube.com" }),
        ];
        for body in refused {
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
        let info = concat!(
            r#"{"title": "Air - 1 - 2026.10.01-16.23.03", "duration": 3, "formats": [{"format_id": "18", "#,
            r#""vcodec": "avc1.42001E", "acodec": "mp4a.40.2", "width": 640, "height": 360, "fps": 30}]}"#
        );
        let error = "ERROR: [youtube] abc: Private video. Sign in if you've been granted access to this video";
        #[cfg(windows)]
        let (file, text) = (
            dir.join(if fail { "fail.cmd" } else { "yt-dlp.cmd" }),
            if fail {
                format!("@echo off\r\necho {error} 1>&2\r\nexit /b 1\r\n")
            } else {
                format!(
                    "@echo off\r\nif \"%~1\"==\"-J\" (\r\necho {info}\r\nexit /b 0\r\n)\r\n:next\r\n\
                     if \"%~1\"==\"\" exit /b 1\r\nif \"%~1\"==\"-o\" (\r\necho aimview 18 5 10 NA\r\n\
                     echo video> \"%~dp2video.mp4\"\r\nexit /b 0\r\n)\r\nshift\r\ngoto next\r\n"
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
                     if [ \"$1\" = \"-o\" ]; then echo 'aimview 18 5 10 NA'; \
                     echo video > \"$(dirname \"$2\")/video.mp4\"; exit 0; fi\n\
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
        let library = Library::open(Config::new(dir.clone(), Layout::App, dir.join("models"))).unwrap();
        ytdlp::set_stand_in(Some(stand_in(&dir, false)));
        let url = json!({ "url": "https://www.youtube.com/watch?v=abc" });
        let formats = library.link_formats(&url).unwrap();
        assert_eq!(formats["title"], "Air - 1 - 2026.10.01-16.23.03");
        assert_eq!(formats["formats"][0]["id"], "18");
        let added = library.add_link(&json!({ "url": url["url"], "format": "18" })).unwrap();
        let id = added["id"].as_str().unwrap().to_string();
        assert_eq!(id, "uploads/Air - 1 - 2026.10.01-16.23.03.mp4");
        assert_eq!(added["recording"]["scenario"], "Air");
        let started = Instant::now();
        while library.job(&id)["stage"] != "none" {
            assert!(library.job(&id)["stage"] != "error", "{}", library.job(&id));
            assert!(started.elapsed().as_secs() < 20, "the download never ended");
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let list = library.recordings(false).unwrap();
        assert!(list.as_array().unwrap().iter().any(|row| row["id"] == id.as_str()), "{list}");
        // only the video: the download's folder is gone
        let names: Vec<String> = std::fs::read_dir(library.uploads())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["Air - 1 - 2026.10.01-16.23.03.mp4"]);
        // the same link again: a name of its own
        let again = library.add_link(&json!({ "url": url["url"], "format": null })).unwrap();
        assert_eq!(again["id"], "uploads/Air - 1 - 2026.10.01-16.23.03 (2).mp4");
        while library.job(again["id"].as_str().unwrap())["stage"] != "none" {
            assert!(started.elapsed().as_secs() < 20, "the download never ended");
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        ytdlp::set_stand_in(Some(stand_in(&dir, true)));
        let failed = library.link_formats(&json!({ "url": "https://www.youtube.com/watch?v=private" })).unwrap_err();
        assert_eq!(failed.status, 400);
        assert_eq!(
            failed.message,
            "yt-dlp cannot read this link: Private video. Sign in if you've been granted access to this video"
        );
        ytdlp::set_stand_in(None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
