//! Reads recordings' HUDs with the core (src/hud.rs) and prints one line of JSON for each:
//! `cargo run --profile quick --example hud -- <video> [more videos]`. ffmpeg decodes (it must be in PATH): first the
//! key frames, picked with ffmpeg's select (some builds ignore `-skip_frame nokey` on AV1), then every frame, which
//! the watch reads as the Y plane of yuv420p. Each recording is read three ways, which must agree: one watch; its part
//! joined into a new watch (as the desktop app and the page join runs); and two runs, each with the next run's first
//! frame, joined. It also prints the time a frame takes and how much the watch keeps.

use std::io::{BufReader, Read};
use std::process::{Child, Command, Stdio};
use std::time::Instant;

use aimview::hud::{HudPart, HudReading, HudWatch};
use serde_json::json;

/// The bytes of one glyph image in a part's JSON: 16 x 24 pixels, two hex digits each.
const GLYPH_HEX_DIGITS: usize = 2 * 16 * 24;
/// The rows a part keeps: KovaaK's Kill Count and Accuracy, Aim Lab's POINTS and TIME.
const HUD_ROWS: usize = 4;

/// A recording's size and range, as ffprobe gives them.
#[derive(Clone, Copy)]
struct Video {
    /// Its width, in pixels.
    width: usize,
    /// Its height, in pixels.
    height: usize,
    /// Whether its Y spans 0..255.
    full_range: bool,
    /// Its packet count (about its frame count).
    packets: usize,
}

impl Video {
    /// The recording's facts from ffprobe. Panics when ffprobe cannot run or gives no size.
    fn probe(path: &str) -> Video {
        let out = Command::new("ffprobe")
            .args(["-v", "error", "-select_streams", "v:0", "-count_packets"])
            .args(["-show_entries", "stream=width,height,color_range,nb_read_packets", "-of", "csv=p=0", path])
            .output()
            .expect("ffprobe runs");
        let text = String::from_utf8_lossy(&out.stdout);
        let fields: Vec<&str> = text.trim().split(',').collect();
        Video {
            width: fields[0].parse().unwrap(),
            height: fields[1].parse().unwrap(),
            full_range: fields[2] == "pc",
            packets: fields[3].parse().unwrap_or(0),
        }
    }

    /// A new HUD watch for a recording of this size and range.
    fn watch(self) -> HudWatch {
        HudWatch::new(self.width, self.height, self.full_range)
    }
}

/// An ffmpeg decoding the video to yuv420p on its standard output: its key frames (`keys`), else every frame.
fn decode(video: &str, keys: bool) -> Child {
    let mut command = Command::new("ffmpeg");
    command.args(["-v", "error", "-i", video]);
    if keys {
        command.args(["-vf", "select=key", "-fps_mode", "passthrough"]);
    }
    command.args(["-f", "rawvideo", "-pix_fmt", "yuv420p", "-"]).stdout(Stdio::piped()).spawn().expect("ffmpeg runs")
}

/// Each frame's Y plane from a decoder, in order.
fn each_frame(child: &mut Child, video: Video, mut each: impl FnMut(&[u8])) {
    let luma_bytes = video.width * video.height;
    let mut reader = BufReader::with_capacity(1 << 22, child.stdout.take().unwrap());
    let mut frame = vec![0u8; luma_bytes * 3 / 2];
    while reader.read_exact(&mut frame).is_ok() {
        each(&frame[..luma_bytes]);
    }
    child.wait().unwrap();
}

/// The parts, each sent through JSON, joined into a new watch: its frame count and its reading.
fn joined(parts: Vec<HudPart>, video: Video) -> (usize, Option<HudReading>) {
    let mut watch = video.watch();
    for part in parts {
        let text = serde_json::to_string(&part).unwrap();
        watch.join(serde_json::from_str(&text).unwrap());
    }
    (watch.frames(), watch.finish())
}

/// The watches a recording is read with: `one` whole, `whole` again for its part, and its two halves, each half with
/// the frame where they meet.
struct Watches {
    /// The watch that reads the whole recording and gives the reading.
    one: HudWatch,
    /// Another watch of the whole recording, whose part is joined into a new watch.
    whole: HudWatch,
    /// The first half, up to the middle frame and with it.
    first: HudWatch,
    /// The second half, from the middle frame on.
    second: HudWatch,
}

/// How a recording's frames were read: their count, and the time the box's layout took (the first frame's add, from
/// the key frames) and every other frame's add took (seconds).
struct FramesRead {
    /// The frames decoded.
    frames: usize,
    /// The first frame's add, which lays out the boxes, in seconds.
    layout_s: f64,
    /// Every other frame's add, summed, in seconds.
    adds_s: f64,
}

/// Every frame into the watches, the halves split at frame `middle`.
fn read_frames(path: &str, video: Video, watches: &mut Watches, middle: usize) -> FramesRead {
    let mut all = decode(path, false);
    let mut read = FramesRead { frames: 0, layout_s: 0.0, adds_s: 0.0 };
    each_frame(&mut all, video, |luma| {
        let added = Instant::now();
        watches.one.add(luma);
        if read.frames == 0 {
            read.layout_s = added.elapsed().as_secs_f64();
        } else {
            read.adds_s += added.elapsed().as_secs_f64();
        }
        watches.whole.add(luma);
        if read.frames <= middle {
            watches.first.add(luma);
        }
        if read.frames >= middle {
            watches.second.add(luma);
        }
        read.frames += 1;
    });
    read
}

/// Each row's runs, distinct lines and their glyphs in a part's JSON (line 0, the empty one, is not in its lines).
fn row_sizes(shape: &serde_json::Value) -> Vec<[usize; 3]> {
    (0..HUD_ROWS)
        .map(|row| {
            let runs = shape["rows"][row].as_array().unwrap();
            let mut used: Vec<u64> = runs.iter().map(|run| run[0].as_u64().unwrap()).filter(|&line| line > 0).collect();
            used.sort_unstable();
            used.dedup();
            let glyphs = used.iter().map(|&line| shape["lines"][line as usize - 1].as_array().unwrap().len()).sum();
            [runs.len(), used.len(), glyphs]
        })
        .collect()
}

/// A recording read three ways, as one line of JSON.
fn read_three_ways(path: &str) -> serde_json::Value {
    let video = Video::probe(path);
    let mut keys = decode(path, true);
    let mut watches = Watches { one: video.watch(), whole: video.watch(), first: video.watch(), second: video.watch() };
    let mut key_count = 0;
    let started = Instant::now();
    each_frame(&mut keys, video, |luma| {
        key_count += 1;
        for watch in [&mut watches.one, &mut watches.whole, &mut watches.first, &mut watches.second] {
            watch.add_key(luma);
        }
    });
    let read = read_frames(path, video, &mut watches, video.packets / 2);
    let Watches { one, whole, first, second } = watches;
    let finishing = Instant::now();
    let reading = one.finish();
    let finish_s = finishing.elapsed().as_secs_f64();
    let part = whole.part();
    let text = serde_json::to_string(&part).unwrap();
    let shape: serde_json::Value = serde_json::from_str(&text).unwrap();
    let images = shape["images"].as_str().map_or(0, |hex| hex.len() / GLYPH_HEX_DIGITS);
    let lines = shape["lines"].as_array().map_or(0, Vec::len);
    let (frames_joined, rejoined) = joined(vec![part], video);
    let (frames_split, split) = joined(vec![first.part(), second.part()], video);
    let frames = read.frames;
    json!({
        "video": path, "width": video.width, "height": video.height, "full": video.full_range, "frames": frames,
        "keys": key_count, "ms_add": 1000.0 * read.adds_s / frames.max(2).saturating_sub(1) as f64,
        "layout_s": read.layout_s, "finish_s": finish_s, "seconds": started.elapsed().as_secs_f64(),
        "part_bytes": text.len(), "images": images, "lines": lines, "rows": row_sizes(&shape),
        "joined_same": rejoined == reading && frames_joined == frames,
        "split_same": split == reading && frames_split == frames,
        "reading": reading,
    })
}

/// Reads each recording named on the command line and prints its line.
fn main() {
    for path in std::env::args().skip(1) {
        println!("{}", read_three_ways(&path));
    }
}
