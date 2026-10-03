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

/// The video's width, height, whether its Y spans 0..255, and its packet count (about its frame count).
fn probe(video: &str) -> (usize, usize, bool, usize) {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", "v:0", "-count_packets"])
        .args(["-show_entries", "stream=width,height,color_range,nb_read_packets", "-of", "csv=p=0", video])
        .output()
        .expect("ffprobe runs");
    let text = String::from_utf8_lossy(&out.stdout);
    let f: Vec<&str> = text.trim().split(',').collect();
    (f[0].parse().unwrap(), f[1].parse().unwrap(), f[2] == "pc", f[3].parse().unwrap_or(0))
}

fn decode(video: &str, keys: bool) -> Child {
    let mut c = Command::new("ffmpeg");
    c.args(["-v", "error", "-i", video]);
    if keys {
        c.args(["-vf", "select=key", "-fps_mode", "passthrough"]);
    }
    c.args(["-f", "rawvideo", "-pix_fmt", "yuv420p", "-"]).stdout(Stdio::piped()).spawn().expect("ffmpeg runs")
}

/// Each frame's Y plane from a decoder, in order.
fn each_frame(child: &mut Child, w: usize, h: usize, mut f: impl FnMut(&[u8])) {
    let mut r = BufReader::with_capacity(1 << 22, child.stdout.take().unwrap());
    let mut buf = vec![0u8; w * h * 3 / 2];
    while r.read_exact(&mut buf).is_ok() {
        f(&buf[..w * h]);
    }
    child.wait().unwrap();
}

fn joined(parts: Vec<HudPart>, w: usize, h: usize, full: bool) -> (usize, Option<HudReading>) {
    let mut watch = HudWatch::new(w, h, full);
    for p in parts {
        let text = serde_json::to_string(&p).unwrap();
        watch.join(serde_json::from_str(&text).unwrap());
    }
    (watch.frames(), watch.finish())
}

fn main() {
    for video in std::env::args().skip(1) {
        let (w, h, full, packets) = probe(&video);
        let mid = packets / 2;
        let mut keys = decode(&video, true);
        let mut all = decode(&video, false);
        let new = || HudWatch::new(w, h, full);
        let (mut one, mut whole, mut first, mut second) = (new(), new(), new(), new());
        let mut key_count = 0;
        let started = Instant::now();
        each_frame(&mut keys, w, h, |y| {
            key_count += 1;
            for watch in [&mut one, &mut whole, &mut first, &mut second] {
                watch.add_key(y);
            }
        });
        // the first frame's time is the box's layout (from the key frames); the others', the reading
        let (mut n, mut spent, mut layout_s) = (0, 0.0, 0.0);
        each_frame(&mut all, w, h, |y| {
            let t = Instant::now();
            one.add(y);
            if n == 0 {
                layout_s = t.elapsed().as_secs_f64();
            } else {
                spent += t.elapsed().as_secs_f64();
            }
            whole.add(y);
            if n <= mid {
                first.add(y);
            }
            if n >= mid {
                second.add(y);
            }
            n += 1;
        });
        let t = Instant::now();
        let reading = one.finish();
        let finish_s = t.elapsed().as_secs_f64();
        let part = whole.part();
        let text = serde_json::to_string(&part).unwrap();
        let shape: serde_json::Value = serde_json::from_str(&text).unwrap();
        let images = shape["images"].as_str().map_or(0, |s| s.len() / 2 / 384);
        let lines = shape["lines"].as_array().map_or(0, |l| l.len());
        // each row's runs, distinct lines and their glyphs (line 0, the empty one, is not in the part's lines)
        let rows: Vec<[usize; 3]> = (0..4)
            .map(|r| {
                let runs = shape["rows"][r].as_array().unwrap();
                let mut used: Vec<u64> = runs.iter().map(|x| x[0].as_u64().unwrap()).filter(|&l| l > 0).collect();
                used.sort_unstable();
                used.dedup();
                let glyphs = used.iter().map(|&l| shape["lines"][l as usize - 1].as_array().unwrap().len()).sum();
                [runs.len(), used.len(), glyphs]
            })
            .collect();
        let (frames_joined, rejoined) = joined(vec![part], w, h, full);
        let (frames_split, split) = joined(vec![first.part(), second.part()], w, h, full);
        let out = json!({
            "video": video, "width": w, "height": h, "full": full, "frames": n, "keys": key_count,
            "ms_add": 1000.0 * spent / n.max(2).saturating_sub(1) as f64, "layout_s": layout_s,
            "finish_s": finish_s, "seconds": started.elapsed().as_secs_f64(),
            "part_bytes": text.len(), "images": images, "lines": lines, "rows": rows,
            "joined_same": rejoined == reading && frames_joined == n,
            "split_same": split == reading && frames_split == n,
            "reading": reading,
        });
        println!("{out}");
    }
}
