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
    Area, AreaFinder, Example, Examples, FRAME, Found, MIN_KEYS, Maps, NEAREST_EXAMPLES, NONE, check, find_json, iou,
    learn_json, predict, sample_frames, session_share,
};
use aimview::convert::{Converter, Matrix};
use aimview::geometry::{H, W};
use aimview::hud::{HudWatch, SessionRows};
use serde_json::{Map, Value, json};

/// ffmpeg's output is read through a buffer this big.
const PIPE_BUFFER_BYTES: usize = 1 << 22;
/// Milliseconds in a second, for the times printed.
const MS_PER_S: f64 = 1000.0;

/// A video as ffprobe gives it: its size, whether its Y spans 0..255, and its duration (format=duration).
struct Video {
    /// The video file, as Python's results name it.
    path: String,
    /// Its width, in pixels.
    width: usize,
    /// Its height, in pixels.
    height: usize,
    /// Whether its Y spans 0..255 (color_range "pc").
    full_range: bool,
    /// Its duration in seconds; 0 when ffprobe gives none.
    duration_s: f64,
}

impl Video {
    /// The video's facts from ffprobe. Panics when ffprobe cannot run or gives no size.
    fn probe(path: &str) -> Video {
        let out = Command::new("ffprobe")
            .args(["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height,color_range"])
            .args(["-show_entries", "format=duration", "-of", "json", path])
            .output()
            .expect("ffprobe runs");
        let probed: Value = serde_json::from_slice(&out.stdout).unwrap();
        let stream = &probed["streams"][0];
        let size = |value: &Value| value.as_u64().unwrap() as usize;
        let duration_s = probed["format"]["duration"].as_str().and_then(|text| text.parse().ok()).unwrap_or(0.0);
        Video {
            path: path.into(),
            width: size(&stream["width"]),
            height: size(&stream["height"]),
            full_range: stream["color_range"] == "pc",
            duration_s,
        }
    }

    /// The bytes of one frame in YUV 4:2:0 at the video's size.
    fn frame_bytes(&self) -> usize {
        self.width * self.height + 2 * self.width.div_ceil(2) * self.height.div_ceil(2)
    }

    /// Each frame ffmpeg decodes (YUV 4:2:0 at the video's size), in order: the key frames or all of them.
    fn each_frame(&self, keys: bool, mut on_frame: impl FnMut(&[u8])) {
        let mut command = Command::new("ffmpeg");
        command.args(["-v", "error", "-i", &self.path]);
        if keys {
            command.args(["-vf", "select=key"]);
        }
        command.args(["-fps_mode", "passthrough", "-f", "rawvideo", "-pix_fmt", "yuv420p", "-"]);
        let mut child = command.stdout(Stdio::piped()).spawn().expect("ffmpeg runs");
        let mut reader = BufReader::with_capacity(PIPE_BUFFER_BYTES, child.stdout.take().unwrap());
        let mut frame = vec![0u8; self.frame_bytes()];
        while reader.read_exact(&mut frame).is_ok() {
            on_frame(&frame);
        }
        child.wait().unwrap();
    }

    /// Every frame's time as ffmpeg's filters see it (from showinfo), in order.
    fn frame_times(&self) -> Vec<f64> {
        let out = Command::new("ffmpeg")
            .args(["-hide_banner", "-i", &self.path, "-vf", "showinfo", "-fps_mode", "passthrough", "-f", "null", "-"])
            .output()
            .expect("ffmpeg runs");
        String::from_utf8_lossy(&out.stderr)
            .lines()
            .filter_map(|line| line.split("pts_time:").nth(1))
            .filter_map(|time| time.split_whitespace().next()?.parse().ok())
            .collect()
    }
}

/// A file's bytes; panics with its path when it cannot be read.
fn read_bytes(path: &str) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|error| panic!("{path}: {error}"))
}

/// A JSON file's value; panics when it cannot be read or is not JSON.
fn read_json(path: &str) -> Value {
    serde_json::from_slice(&read_bytes(path)).unwrap()
}

/// [x0, y0, x1, y1, kind] boxes as numbers and a kind, to compare Python's with the core's.
fn boxes(value: &Value) -> Vec<(Vec<f64>, String)> {
    let list = value.as_array().map(Vec::as_slice).unwrap_or_default();
    list.iter()
        .map(|entry| {
            let entry = entry.as_array().unwrap();
            (
                entry[..4].iter().map(|share| share.as_f64().unwrap()).collect(),
                entry.get(4).and_then(Value::as_str).unwrap_or("").into(),
            )
        })
        .collect()
}

/// The pure parts against Python's: check, each example's guess, find and learn.
fn pure(dir: &str, refs: &str, names: Vec<String>) {
    let text = String::from_utf8(read_bytes(&format!("{dir}/examples.jsonl"))).unwrap();
    println!("{}", check_learner(dir, &Examples::Lines(text.clone()).list()));
    for name in names {
        println!("{}", json!({"name": name, "same": find_and_learn(dir, refs, &name, &text)}));
    }
}

/// `check`, and each example's guess with its recording left out, against Python's (check.json).
fn check_learner(dir: &str, examples: &[Example]) -> Value {
    let python = read_json(&format!("{dir}/check.json"));
    let checked = check(examples);
    let wrong: Vec<Value> = checked.wrong.iter().map(|(truth, guess, count)| json!([truth, guess, count])).collect();
    let guesses: Vec<String> = examples.iter().map(|example| guess_left_out(example, examples)).collect();
    let python_guesses: Vec<&str> =
        python["guesses"].as_array().unwrap().iter().map(|guess| guess.as_str().unwrap()).collect();
    let differ: Vec<usize> = (0..guesses.len()).filter(|&i| guesses[i] != python_guesses[i]).collect();
    json!({"check": {"sure": checked.sure, "right": checked.right, "count": checked.count,
            "py_sure": python["sure"], "py_right": python["right"], "py_count": python["count"],
            "same_wrong": Value::from(wrong) == python["wrong"]},
        "guesses": guesses.len(), "guesses_differ": differ})
}

/// An example's kind as the learner guesses it from the other recordings' examples ("none": left out).
fn guess_left_out(example: &Example, examples: &[Example]) -> String {
    let rest: Vec<Example> = examples.iter().filter(|other| other.rec != example.rec).cloned().collect();
    let area = Area { bounds: [0.0; 4], feat: example.feat, rule: "?".into() };
    predict(&[area], &rest, NEAREST_EXAMPLES).first().map_or(NONE.to_string(), |named| named.kind.clone())
}

/// `find_json` (copying a labelled layout, and fresh) and `learn_json` (with the maps and without) on one recording,
/// each against Python's (<name>.json, the maps from the reference folder).
fn find_and_learn(dir: &str, refs: &str, name: &str, examples: &str) -> Map<String, Value> {
    let python = read_json(&format!("{dir}/{name}.json"));
    let input = &python["input"];
    let mut same = Map::new();
    for (key, labelled) in [("copy", input["labelled"].clone()), ("fresh", json!([]))] {
        let request = json!({"found": input["found"], "examples": examples, "labelled": labelled});
        let out: Value = serde_json::from_str(&find_json(&request.to_string()).unwrap()).unwrap();
        let expected = &python[key];
        let equal = boxes(&out["boxes"]) == boxes(&expected["boxes"])
            && out["copied"] == expected["copied"]
            && out["by"] == expected["by"];
        same.insert(key.into(), json!(equal));
    }
    let maps =
        Maps::new(read_bytes(&format!("{refs}/{name}.stand.bin")), read_bytes(&format!("{refs}/{name}.change.bin")))
            .unwrap();
    for (key, maps) in [("learn_maps", Some(&maps)), ("learn", None)] {
        let request =
            json!({"rec": name, "found": input["found"], "saved": python["saved"], "maps": maps, "examples": examples});
        let out: Value = serde_json::from_str(&learn_json(&request.to_string()).unwrap()).unwrap();
        same.insert(key.into(), json!(out["examples"] == python[key]));
    }
    same
}

/// Checks each named recording against the reference folder, or with `pure` first, the pure parts.
fn main() {
    let mut args = std::env::args().skip(1);
    let dir = args.next().expect("a reference folder");
    if dir == "pure" {
        let (dir, refs) = (args.next().expect("a folder"), args.next().expect("a reference folder"));
        return pure(&dir, &refs, args.collect());
    }
    for name in args {
        println!("{}", check_recording(&dir, &name));
    }
}

/// What the finder reads from a recording: its key frames' count, the frames it is given (each scaled to 1280 x 720
/// YUV 4:2:0, as the review does), and the HUD watch that saw the key frames.
struct Frames {
    /// The recording's key frames.
    keys: usize,
    /// The frames the finder is given, each 1280 x 720 YUV 4:2:0.
    frames: Vec<Vec<u8>>,
    /// The HUD watch, which saw every key frame, for KovaaK's session box.
    hud: HudWatch,
}

/// The key frames (picked with select, as examples/hud.rs does), and, for a run with fewer than MIN_KEYS of them,
/// every frame, from which the frames `sample_frames` picks are taken.
fn read_frames(video: &Video, name: &str) -> Frames {
    let mut convert = Converter::new(video.width, video.height, Matrix::Bt709, video.full_range);
    let mut hud = HudWatch::new(video.width, video.height, video.full_range);
    let mut small = vec![0u8; FRAME];
    let mut key_frames: Vec<Vec<u8>> = Vec::new();
    video.each_frame(true, |yuv| {
        hud.add_key(&yuv[..video.width * video.height]);
        convert.yuv420p(yuv, &mut small);
        key_frames.push(small.clone());
    });
    let times = if key_frames.len() < MIN_KEYS { video.frame_times() } else { Vec::new() };
    let frames = match sample_frames(key_frames.len(), &times, video.duration_s) {
        None => key_frames.clone(),
        Some(picks) => {
            let mut all = Vec::new();
            video.each_frame(false, |yuv| {
                convert.yuv420p(yuv, &mut small);
                all.push(small.clone());
            });
            assert_eq!(all.len(), times.len(), "{name}: showinfo and the decode differ");
            picks.iter().map(|&i| all[i].clone()).collect()
        }
    };
    Frames { keys: key_frames.len(), frames, hud }
}

/// What the finder found, KovaaK's box it was given, and its times: per frame added, and for finish.
struct Finding {
    /// What `finish` gave: the areas and the maps.
    found: Found,
    /// KovaaK's session box as the HUD watch found it; None without one.
    session: Option<SessionRows>,
    /// The mean time to add a frame, in milliseconds.
    add_ms: f64,
    /// The time `finish` took, in milliseconds.
    finish_ms: f64,
}

/// Runs the finder over the frames, timing each step.
fn find(frames: &mut Frames) -> Finding {
    let mut finder = AreaFinder::new();
    let started = Instant::now();
    for frame in &frames.frames {
        finder.add(frame);
    }
    let add_ms = MS_PER_S * started.elapsed().as_secs_f64() / frames.frames.len().max(1) as f64;
    let session = frames.hud.session_box();
    let finishing = Instant::now();
    let found: Found = finder.finish(session);
    let finish_ms = MS_PER_S * finishing.elapsed().as_secs_f64();
    Finding { found, session, add_ms, finish_ms }
}

/// How many of the maps' bytes, and of the change sums over the frames, differ from Python's.
fn maps_differ(base: &str, found: &Found, frames: &[Vec<u8>]) -> [usize; 3] {
    let differ = |a: &[u8], b: &[u8]| a.iter().zip(b).filter(|(x, y)| x != y).count();
    let python_stand = read_bytes(&format!("{base}.stand.bin"));
    let python_change = read_bytes(&format!("{base}.change.bin"));
    let python_sums: Vec<u32> = read_bytes(&format!("{base}.sums.bin"))
        .chunks_exact(4)
        .map(|bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        .collect();
    let mut sums = vec![0u32; W * H];
    for pair in frames.windows(2) {
        for (i, sum) in sums.iter_mut().enumerate() {
            *sum += pair[0][i].abs_diff(pair[1][i]) as u32;
        }
    }
    [
        differ(found.maps.stand(), &python_stand),
        differ(found.maps.change(), &python_change),
        sums.iter().zip(&python_sums).filter(|(a, b)| a != b).count(),
    ]
}

/// Each of Python's areas matched to the core's best overlap, and the rules of the core's areas left over.
fn match_areas<'a>(python_found: &[Value], found: &'a Found) -> (Vec<Value>, Vec<&'a str>) {
    let bounds = |area: &Value| -> [f64; 4] {
        let shares = area["box"].as_array().unwrap();
        [0, 1, 2, 3].map(|i| shares[i].as_f64().unwrap())
    };
    let mut matched = Vec::new();
    let mut used = vec![false; found.areas.len()];
    for python_area in python_found {
        let python_bounds = bounds(python_area);
        let best = found
            .areas
            .iter()
            .enumerate()
            .map(|(i, area)| (iou(&python_bounds, &area.bounds), i))
            .max_by(|a, b| a.0.total_cmp(&b.0));
        let feat: Vec<f64> =
            python_area["feat"].as_array().unwrap().iter().map(|value| value.as_f64().unwrap()).collect();
        let rule = &python_area["rule"];
        match best {
            Some((overlap, i)) if overlap > 0.0 => {
                used[i] = true;
                let area = &found.areas[i];
                let feat_diff = feat.iter().zip(&area.feat).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max);
                matched.push(json!({"iou": (overlap * 1e4).round() / 1e4, "python": rule, "core": area.rule,
                    "same_box": area.bounds == python_bounds, "feat_diff": feat_diff}));
            }
            _ => matched.push(json!({"iou": 0, "python": rule, "core": null})),
        }
    }
    let extra = found.areas.iter().zip(&used).filter(|&(_, &is_used)| !is_used).map(|(area, _)| area.rule.as_str());
    (matched, extra.collect())
}

/// One recording against Python's results (<name>.json and its maps): one line of JSON.
fn check_recording(dir: &str, name: &str) -> Value {
    let base = format!("{dir}/{name}");
    let reference = read_json(&format!("{base}.json"));
    let video = Video::probe(reference["video"].as_str().unwrap());
    let mut frames = read_frames(&video, name);
    let Finding { found, session, add_ms, finish_ms } = find(&mut frames);
    let [stand_differ, change_differ, sums_differ] = maps_differ(&base, &found, &frames.frames);
    let text = serde_json::to_string(&found).unwrap();
    let back: Found = serde_json::from_str(&text).unwrap();
    let maps_text = serde_json::to_string(&found.maps).unwrap();
    let maps_value: Value = serde_json::from_str(&maps_text).unwrap();
    let plane_bytes = |key: &str| maps_value[key].as_str().map_or(0, |base64| base64.len() * 3 / 4);
    let python_found = reference["found"].as_array().unwrap();
    let (matched, extra) = match_areas(python_found, &found);
    let all_same = matched
        .iter()
        .all(|area| area["same_box"] == true && area["python"] == area["core"] && area["feat_diff"] == 0.0)
        && extra.is_empty()
        && found.areas.len() == python_found.len();
    // hud.layout's rows, then x0 and x1
    let layout = reference["layout"].as_array().map(|layout| {
        let len = layout.len();
        json!([layout[len - 2], layout[0][0], layout[len - 1], layout[len - 3][1]])
    });
    json!({
        "name": name, "keys": frames.keys, "py_keys": reference["keys"], "frames": found.frames,
        "py_frames": reference["frames"],
        "stand_differ": stand_differ, "change_differ": change_differ, "sums_differ": sums_differ,
        "session": session.map(|rows| [rows.x0, rows.y0, rows.x1, rows.y1]), "py_session": layout,
        "session_share": session.map(session_share),
        "areas": found.areas.len(), "py_areas": python_found.len(), "all_same": all_same,
        "matched": matched, "extra": extra,
        "ms_add": add_ms, "ms_finish": finish_ms, "found_bytes": text.len(), "maps_bytes": maps_text.len(),
        "packed": [plane_bytes("stand"), plane_bytes("change")],
        "round_trip": back == found,
    })
}
