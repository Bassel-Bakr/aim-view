//! Checks the area finder (src/areas.rs) against python/areas.py's analyse() on recordings:
//! `cargo run --profile quick --example areas -- <reference folder> <name> [more names]`. The folder holds, for each
//! name, Python's results: <name>.json (the video, its key frames, the frames read, the found areas, hud.layout's rows)
//! and its maps as raw bytes (<name>.stand.bin and <name>.change.bin, u8 1280 x 720; <name>.sums.bin, the change
//! map's sums, u32). ffmpeg decodes (it must be in PATH): the key frames (picked with select, as examples/hud.rs does),
//! and, for a run with fewer than 24 of them, every frame, from which the frames areas::sample_frames picks are read.
//! The core's Converter scales each to 1280 x 720 YUV 4:2:0, as the review does. Prints one line of JSON for each
//! recording: the maps' differences, each Python area matched to the core's by IoU with both kinds, the time a frame
//! takes and the size of what finish() gives.
//!
//! `-- pure <folder> <name> [more names]` checks the pure parts against Python's on the user's data instead: the
//! folder holds examples.jsonl (a copy of area_examples.jsonl), check.json (check(), and each example's guess with its
//! recording left out) and, for each name, <name>.json (find() with and without copying, and learn() with and without
//! the maps, which are read from the reference folder's <name>.stand.bin and <name>.change.bin).

use std::io::{BufReader, Read};
use std::process::{Command, Stdio};
use std::time::Instant;

use aimview::areas::{
    Area, AreaFinder, Examples, FRAME, Found, K, Maps, NONE, check, find_json, iou, learn_json, predict, sample_frames,
    session_share,
};
use aimview::convert::{Converter, Matrix};
use aimview::geometry::{H, W};
use aimview::hud::HudWatch;
use serde_json::{Value, json};

/// The video's width, height, whether its Y spans 0..255, and its duration (ffprobe's format=duration).
fn probe(video: &str) -> (usize, usize, bool, f64) {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height,color_range"])
        .args(["-show_entries", "format=duration", "-of", "json", video])
        .output()
        .expect("ffprobe runs");
    let j: Value = serde_json::from_slice(&out.stdout).unwrap();
    let s = &j["streams"][0];
    let num = |v: &Value| v.as_u64().unwrap() as usize;
    let duration = j["format"]["duration"].as_str().and_then(|d| d.parse().ok()).unwrap_or(0.0);
    (num(&s["width"]), num(&s["height"]), s["color_range"] == "pc", duration)
}

/// Each frame ffmpeg decodes (YUV 4:2:0 at the video's size), in order: the key frames or all of them.
fn each_frame(video: &str, keys: bool, size: usize, mut f: impl FnMut(&[u8])) {
    let mut c = Command::new("ffmpeg");
    c.args(["-v", "error", "-i", video]);
    if keys {
        c.args(["-vf", "select=key"]);
    }
    c.args(["-fps_mode", "passthrough", "-f", "rawvideo", "-pix_fmt", "yuv420p", "-"]);
    let mut child = c.stdout(Stdio::piped()).spawn().expect("ffmpeg runs");
    let mut r = BufReader::with_capacity(1 << 22, child.stdout.take().unwrap());
    let mut buf = vec![0u8; size];
    while r.read_exact(&mut buf).is_ok() {
        f(&buf);
    }
    child.wait().unwrap();
}

/// Every frame's time as ffmpeg's filters see it (from showinfo), in order.
fn frame_times(video: &str) -> Vec<f64> {
    let out = Command::new("ffmpeg")
        .args(["-hide_banner", "-i", video, "-vf", "showinfo", "-fps_mode", "passthrough", "-f", "null", "-"])
        .output()
        .expect("ffmpeg runs");
    String::from_utf8_lossy(&out.stderr)
        .lines()
        .filter_map(|l| l.split("pts_time:").nth(1))
        .filter_map(|t| t.split_whitespace().next()?.parse().ok())
        .collect()
}

fn read_bytes(path: &str) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// [x0, y0, x1, y1, kind] boxes as numbers and a kind, to compare Python's with the core's.
fn boxes(v: &Value) -> Vec<(Vec<f64>, String)> {
    let list = v.as_array().map(Vec::as_slice).unwrap_or_default();
    list.iter()
        .map(|b| {
            let b = b.as_array().unwrap();
            (
                b[..4].iter().map(|x| x.as_f64().unwrap()).collect(),
                b.get(4).and_then(Value::as_str).unwrap_or("").into(),
            )
        })
        .collect()
}

/// The pure parts against Python's: check, each example's guess, find and learn.
fn pure(dir: &str, refs: &str, names: Vec<String>) {
    let text = String::from_utf8(read_bytes(&format!("{dir}/examples.jsonl"))).unwrap();
    let ex = Examples::Lines(text.clone()).list();
    let py: Value = serde_json::from_slice(&read_bytes(&format!("{dir}/check.json"))).unwrap();
    let c = check(&ex);
    let wrong: Vec<Value> = c.wrong.iter().map(|(t, g, n)| json!([t, g, n])).collect();
    let guesses: Vec<String> = ex
        .iter()
        .map(|e| {
            let rest: Vec<_> = ex.iter().filter(|o| o.rec != e.rec).cloned().collect();
            let one = Area { bounds: [0.0; 4], feat: e.feat.clone(), rule: "?".into() };
            predict(&[one], &rest, K).first().map_or(NONE.to_string(), |a| a.kind.clone())
        })
        .collect();
    let py_guesses: Vec<&str> = py["guesses"].as_array().unwrap().iter().map(|g| g.as_str().unwrap()).collect();
    let differ: Vec<usize> = (0..guesses.len()).filter(|&i| guesses[i] != py_guesses[i]).collect();
    println!(
        "{}",
        json!({"check": {"sure": c.sure, "right": c.right, "count": c.count, "py_sure": py["sure"], "py_right": py["right"],
            "py_count": py["count"], "same_wrong": Value::from(wrong) == py["wrong"]},
            "guesses": guesses.len(), "guesses_differ": differ})
    );
    for name in names {
        let py: Value = serde_json::from_slice(&read_bytes(&format!("{dir}/{name}.json"))).unwrap();
        let input = &py["input"];
        let mut same = serde_json::Map::new();
        for (key, labelled) in [("copy", input["labelled"].clone()), ("fresh", json!([]))] {
            let req = json!({"found": input["found"], "examples": text, "labelled": labelled});
            let out: Value = serde_json::from_str(&find_json(&req.to_string()).unwrap()).unwrap();
            let ok = boxes(&out["boxes"]) == boxes(&py[key]["boxes"])
                && out["copied"] == py[key]["copied"]
                && out["by"] == py[key]["by"];
            same.insert(key.into(), json!(ok));
        }
        let maps = Maps::new(
            read_bytes(&format!("{refs}/{name}.stand.bin")),
            read_bytes(&format!("{refs}/{name}.change.bin")),
        )
        .unwrap();
        for (key, maps) in [("learn_maps", Some(&maps)), ("learn", None)] {
            let req =
                json!({"rec": name, "found": input["found"], "saved": py["saved"], "maps": maps, "examples": text});
            let out: Value = serde_json::from_str(&learn_json(&req.to_string()).unwrap()).unwrap();
            same.insert(key.into(), json!(out["examples"] == py[key]));
        }
        println!("{}", json!({"name": name, "same": same}));
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = args.next().expect("a reference folder");
    if dir == "pure" {
        let (dir, refs) = (args.next().expect("a folder"), args.next().expect("a reference folder"));
        return pure(&dir, &refs, args.collect());
    }
    for name in args {
        let base = format!("{dir}/{name}");
        let reference: Value = serde_json::from_slice(&read_bytes(&format!("{base}.json"))).unwrap();
        let video = reference["video"].as_str().unwrap().to_string();
        let (w, h, full, duration) = probe(&video);
        let size = w * h + 2 * w.div_ceil(2) * h.div_ceil(2);
        let mut convert = Converter::new(w, h, Matrix::Bt709, full);
        let mut hud = HudWatch::new(w, h, full);
        let mut small = vec![0u8; FRAME];
        let mut key_frames: Vec<Vec<u8>> = Vec::new();
        each_frame(&video, true, size, |yuv| {
            hud.add_key(&yuv[..w * h]);
            convert.yuv420p(yuv, &mut small);
            key_frames.push(small.clone());
        });
        // the frames the finder reads: the key frames, or those the fps filter would pick
        let times = if key_frames.len() < 24 { frame_times(&video) } else { Vec::new() };
        let frames = match sample_frames(key_frames.len(), &times, duration) {
            None => key_frames.clone(),
            Some(picks) => {
                let mut all = Vec::new();
                each_frame(&video, false, size, |yuv| {
                    convert.yuv420p(yuv, &mut small);
                    all.push(small.clone());
                });
                assert_eq!(all.len(), times.len(), "{name}: showinfo and the decode differ");
                picks.iter().map(|&i| all[i].clone()).collect()
            }
        };
        let mut finder = AreaFinder::new();
        let started = Instant::now();
        for f in &frames {
            finder.add(f);
        }
        let add_ms = 1000.0 * started.elapsed().as_secs_f64() / frames.len().max(1) as f64;
        let session = hud.session_box();
        let t = Instant::now();
        let found: Found = finder.finish(session);
        let finish_ms = 1000.0 * t.elapsed().as_secs_f64();

        // the maps
        let differ = |a: &[u8], b: &[u8]| a.iter().zip(b).filter(|(x, y)| x != y).count();
        let py_stand = read_bytes(&format!("{base}.stand.bin"));
        let py_change = read_bytes(&format!("{base}.change.bin"));
        let py_sums: Vec<u32> = read_bytes(&format!("{base}.sums.bin"))
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();
        let mut sums = vec![0u32; W * H];
        for pair in frames.windows(2) {
            for (i, s) in sums.iter_mut().enumerate() {
                *s += pair[0][i].abs_diff(pair[1][i]) as u32;
            }
        }
        let text = serde_json::to_string(&found).unwrap();
        let back: Found = serde_json::from_str(&text).unwrap();
        let maps_text = serde_json::to_string(&found.maps).unwrap();
        let maps_value: Value = serde_json::from_str(&maps_text).unwrap();
        let plane_bytes = |k: &str| maps_value[k].as_str().map_or(0, |s| s.len() * 3 / 4);

        // the areas: each of Python's matched to the core's best overlap
        let py_found = reference["found"].as_array().unwrap();
        let bounds = |a: &Value| -> [f64; 4] {
            let b = a["box"].as_array().unwrap();
            [0, 1, 2, 3].map(|i| b[i].as_f64().unwrap())
        };
        let mut matched = Vec::new();
        let mut used = vec![false; found.areas.len()];
        for a in py_found {
            let pb = bounds(a);
            let best = found
                .areas
                .iter()
                .enumerate()
                .map(|(i, r)| (iou(&pb, &r.bounds), i))
                .max_by(|x, y| x.0.total_cmp(&y.0));
            let feat: Vec<f64> = a["feat"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
            match best {
                Some((v, i)) if v > 0.0 => {
                    used[i] = true;
                    let r = &found.areas[i];
                    let feat_diff = feat.iter().zip(&r.feat).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max);
                    matched.push(json!({"iou": (v * 1e4).round() / 1e4, "python": a["rule"], "core": r.rule,
                        "same_box": r.bounds == pb, "feat_diff": feat_diff}));
                }
                _ => matched.push(json!({"iou": 0, "python": a["rule"], "core": null})),
            }
        }
        let extra: Vec<&str> =
            found.areas.iter().zip(&used).filter(|(_, u)| !**u).map(|(a, _)| a.rule.as_str()).collect();
        let layout = reference["layout"].as_array().map(|l| {
            let n = l.len();
            json!([l[n - 2], l[0][0], l[n - 1], l[n - 3][1]])
        });
        let out = json!({
            "name": name, "keys": key_frames.len(), "py_keys": reference["keys"], "frames": found.frames,
            "py_frames": reference["frames"],
            "stand_differ": differ(found.maps.stand(), &py_stand), "change_differ": differ(found.maps.change(), &py_change),
            "sums_differ": sums.iter().zip(&py_sums).filter(|(a, b)| a != b).count(),
            "session": session.map(|s| [s.x0, s.y0, s.x1, s.y1]), "py_session": layout,
            "session_share": session.map(session_share),
            "areas": found.areas.len(), "py_areas": py_found.len(),
            "all_same": matched.iter().all(|m| m["same_box"] == true && m["python"] == m["core"] && m["feat_diff"] == 0.0)
                && extra.is_empty() && found.areas.len() == py_found.len(),
            "matched": matched, "extra": extra,
            "ms_add": add_ms, "ms_finish": finish_ms, "found_bytes": text.len(), "maps_bytes": maps_text.len(),
            "packed": [plane_bytes("stand"), plane_bytes("change")],
            "round_trip": back == found,
        });
        println!("{out}");
    }
}
