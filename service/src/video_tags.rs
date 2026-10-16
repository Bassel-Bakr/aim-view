//! A video's container-level metadata tags, whatever its container (MP4, MOV, MKV, WebM, FLV...), from the demuxers
//! the app already has: ffprobe natively (video.rs), and in the browser build the page's Mediabunny (disk.rs's
//! `video_tags`). A tag's key is matched ignoring case: ffmpeg writes a Matroska file's keys in upper case. KovOBS
//! keeps the run's KovaaK's stats file in one (library/stats.rs).
//!
//! In: the video's path. Out: one tag's value, as text.

use std::collections::HashMap;
use std::io;
use std::path::Path;

/// The value of the tag `key` (any case) among the video's container-level tags; None when it has no such tag, or
/// the demuxer cannot read the video. An error when the demuxer cannot be reached (no ffprobe to run).
pub fn tag(video: &Path, key: &str) -> io::Result<Option<String>> {
    Ok(tags(video)?.into_iter().find(|(name, _)| name.eq_ignore_ascii_case(key)).map(|(_, value)| value))
}

/// The video's container-level tags, from ffprobe. With no ffprobe to run yet (the desktop app downloads ffmpeg), its
/// download starts and this asking fails: the tags are read when asked again.
#[cfg(feature = "native")]
fn tags(video: &Path) -> io::Result<HashMap<String, String>> {
    let answer = crate::video::tags(video);
    if answer.as_ref().is_err_and(|error| error.kind() == io::ErrorKind::NotFound) {
        fetch_ffmpeg();
    }
    answer
}

/// Starts ffmpeg's download in the background, once a process (ffmpeg.rs `ensure`: only when it is to be downloaded
/// and the PATH has none), so the recordings list and the report never wait for it as a review does.
#[cfg(feature = "native")]
fn fetch_ffmpeg() {
    /// Set once the download has started.
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        std::thread::spawn(|| {
            let _ = crate::ffmpeg::ensure(|_, _| {});
        });
    });
}

/// The video's container-level tags, from the page's Mediabunny.
#[cfg(not(feature = "native"))]
fn tags(video: &Path) -> io::Result<HashMap<String, String>> {
    let answer = crate::disk::video_tags(video)?;
    serde_json::from_slice(&answer)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, format!("the page's video tags: {error}")))
}

#[cfg(all(test, feature = "native"))]
pub(crate) mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::process::Command;

    /// A one-frame video at `path` made by ffmpeg with the container-level `tags`; its codec and flags fit the
    /// container its extension names (an MP4's or a MOV's keys need `use_metadata_tags`). The same tags' lengths give
    /// the same file size: the encode is bit-exact.
    pub(crate) fn tagged_video(path: &Path, tags: &[(&str, &str)]) {
        let extension = path.extension().map(|extension| extension.to_string_lossy().to_lowercase());
        let codec = if extension.as_deref() == Some("flv") { "flv1" } else { "mpeg4" };
        let mut command = Command::new(crate::ffmpeg::program("ffmpeg"));
        command.args([
            "-v",
            "error",
            "-nostdin",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc=d=0.04:s=16x16:r=25",
            "-threads",
            "1",
        ]);
        command.args(["-c:v", codec, "-fflags", "+bitexact", "-flags:v", "+bitexact"]);
        if matches!(extension.as_deref(), Some("mp4" | "mov")) {
            command.args(["-movflags", "use_metadata_tags"]);
        }
        for (key, value) in tags {
            command.arg("-metadata").arg(format!("{key}={value}"));
        }
        let status = command.arg(path).status().expect("ffmpeg runs");
        assert!(status.success(), "ffmpeg made {}", path.display());
    }

    /// A test's own temporary folder, made empty.
    fn test_dir(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aimview-video-tags-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A tag is found in every container that keeps it, by its key in any case (a Matroska file's is upper case); an
    /// MP4 made without `use_metadata_tags`, an MPEG-TS (both drop it), or a file that is no video gives none.
    #[test]
    fn a_tag_is_found_in_any_container_that_keeps_it() {
        let dir = test_dir("containers");
        let csv = "Kill #,Timestamp\r\n1,17:43:05.387\r\nScore:,60120\r\n";
        for name in ["a.mp4", "a.mov", "a.mkv", "a.flv"] {
            tagged_video(&dir.join(name), &[("kovobs_kovaaks_stats", csv)]);
            assert_eq!(tag(&dir.join(name), "kovobs_kovaaks_stats").unwrap().as_deref(), Some(csv), "{name}");
            assert_eq!(tag(&dir.join(name), "KOVOBS_KOVAAKS_STATS").unwrap().as_deref(), Some(csv), "{name}");
            assert_eq!(tag(&dir.join(name), "other").unwrap(), None, "{name}");
        }
        let plain = dir.join("plain.mp4");
        let status = Command::new(crate::ffmpeg::program("ffmpeg"))
            .args(["-v", "error", "-nostdin", "-y", "-f", "lavfi", "-i", "testsrc=d=0.04:s=16x16", "-c:v", "mpeg4"])
            .args(["-metadata", "kovobs_kovaaks_stats=x"])
            .arg(&plain)
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(tag(&plain, "kovobs_kovaaks_stats").unwrap(), None, "an MP4 drops it without use_metadata_tags");
        tagged_video(&dir.join("a.ts"), &[("kovobs_kovaaks_stats", csv)]);
        assert_eq!(tag(&dir.join("a.ts"), "kovobs_kovaaks_stats").unwrap(), None, "an MPEG-TS drops it");
        std::fs::write(dir.join("text.mp4"), csv).unwrap();
        assert_eq!(tag(&dir.join("text.mp4"), "kovobs_kovaaks_stats").unwrap(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
