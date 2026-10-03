//! The faint-target cut-off (python/server.py: faint, set_faint, submit_faint, skip_faint, faint_queue): the user's
//! cut-off for a recording (faint.json in its folder), which a tracking run's report measures with (report.rs), and a
//! submitted one written as detector labels (the core picks the crops, python/model/hand_crops.py's `cutoff_crops`;
//! here each crop's pixels and the fixed map are read, and written as that script writes them).

use std::path::{Path, PathBuf};

use aimview::convert::{Converter, DST_H as H, DST_W as W};
use aimview::faint::{CROP, CutoffCrop, CutoffRequest, DEFAULT_OFFSET, cutoff_crops};
use aimview::track::Tracks;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::library::{Answer, Failure, Library, local_stamp};
use crate::npz::{self, Array};
use crate::pyjson;
use crate::video::{Frames, VideoInfo, probe};

const FAINT: &str = "faint.json";
const FAINT_SKIPPED: &str = "faint_skipped.json";

/// faint.json as the review server writes it: on and offset, and once submitted when and how many labels it gave.
#[derive(Serialize, Deserialize)]
struct FaintFile {
    on: bool,
    offset: f64,
    #[serde(flatten)]
    record: Option<Submitted>,
}

#[derive(Serialize, Deserialize)]
struct Submitted {
    submitted: String,
    #[serde(default)]
    labels: Value,
}

/// Python's truth of a value (`bool(v)`).
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64() != Some(0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// The time now as Python's `datetime.now().isoformat(timespec="seconds")`.
fn now_iso() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64());
    let s = local_stamp(secs.floor());
    format!("{}-{}-{}T{}:{}:{}", &s[0..4], &s[5..7], &s[8..10], &s[11..13], &s[14..16], &s[17..19])
}

impl Library {
    fn faint_path(&self, id: &str) -> PathBuf {
        self.review_dir(id).join(FAINT)
    }

    /// The recording's cut-off as kept ({on, offset}, and submitted and labels once submitted); off by default.
    pub fn faint(&self, id: &str) -> Value {
        pyjson::load(&self.faint_path(id)).unwrap_or_else(|| json!({ "on": false, "offset": DEFAULT_OFFSET }))
    }

    fn faint_file(&self, id: &str) -> Option<FaintFile> {
        serde_json::from_value(pyjson::load(&self.faint_path(id))?).ok()
    }

    /// Keeps the cut-off ({on, offset}); a later change keeps the record of the last submit. The report measures with it
    /// when it is next shown.
    pub fn set_faint(&self, id: &str, body: &Value, submitted: Option<String>) -> Answer<Value> {
        let on = truthy(&body["on"]);
        let offset = match body.get("offset") {
            None => DEFAULT_OFFSET,
            Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
            Some(Value::String(s)) => s.trim().parse().map_err(|_| Failure::bad(format!("could not convert string to float: '{s}'")))?,
            Some(Value::Bool(b)) => f64::from(u8::from(*b)),
            Some(v) => return Err(Failure::bad(format!("offset must be a number, not {v}"))),
        };
        if !(0.2..=0.6).contains(&offset) {
            return Err(Failure::bad("offset must be between 0.2 and 0.6"));
        }
        let old = self.faint(id);
        let record = if submitted.is_some() || truthy(&old["submitted"]) {
            Some(match submitted {
                Some(at) => Submitted { submitted: at, labels: Value::Null },
                None => Submitted {
                    submitted: old["submitted"].as_str().map_or_else(|| old["submitted"].to_string(), str::to_string),
                    labels: old.get("labels").cloned().unwrap_or(Value::Null),
                },
            })
        } else {
            None
        };
        let new = FaintFile { on, offset: aimview::python::round(offset, 2), record };
        pyjson::dump(&self.faint_path(id), &new, false)?;
        Ok(self.faint(id))
    }

    /// The user's cut-off, submitted: kept (on), and the review's tracks written as detector labels in the background.
    pub fn submit_faint(&self, id: &str, offset: f64) -> Answer<Value> {
        let dir = self.shown(id).1;
        let report = match self.report(id) {
            Ok(r) if dir.join("tracks.json").is_file() && !r.is_null() => r,
            Ok(_) => return Err(Failure::bad("review the recording first")),
            Err(f) => return Err(f),
        };
        let out = self.set_faint(id, &json!({ "on": true, "offset": offset }), Some(now_iso()))?;
        let video: PathBuf = self.resolve(id)?.components().collect();
        let exclude: Vec<[f64; 4]> = self.exclude_areas(id);
        // the labels go where python/ keeps them (the layout's cutoff folder): crops in train/, rows in checked.jsonl
        let (faint, labels) = (self.faint_path(id), self.folders().cutoff.clone());
        std::thread::spawn(move || {
            let n = match cutoff_labels(&video, &dir.join("tracks.json"), &report, exclude, offset, &labels) {
                Ok(n) => n,
                Err(e) => {
                    eprintln!("the cut-off's labels: {e}");
                    return;
                }
            };
            let Some(mut f) = pyjson::load(&faint).and_then(|v| serde_json::from_value::<FaintFile>(v).ok()) else { return };
            if let Some(r) = f.record.as_mut() {
                r.labels = json!(n);
            }
            if let Err(e) = pyjson::dump(&faint, &f, false) {
                eprintln!("the cut-off's labels: {e}");
            }
        });
        Ok(out)
    }

    /// Skipped in the cut-off queue: left out of it from now on.
    pub fn skip_faint(&self, id: &str) -> Answer<Value> {
        crate::labels::add_id(&self.file(FAINT_SKIPPED), id)
    }

    /// Recordings to set a cut-off in, in the area queue's order, leaving out probes, other games, skipped and
    /// submitted ones.
    pub fn faint_queue(&self) -> Answer<Value> {
        let skipped = crate::labels::read_ids(&self.file(FAINT_SKIPPED));
        let submitted = |id: &str| self.faint_file(id).and_then(|f| f.record).is_some_and(|r| !r.submitted.is_empty());
        Ok(json!(self.queue(|id| skipped.contains(id) || submitted(id))?))
    }
}

/// The labels of a submitted cut-off (python/server.py: submit_faint's run): the crops the core picks from the run's
/// frames, each written as hand_crops.py writes them (train/<stem>_<frame>.npz: the crop's RGB and fixed map, an empty
/// target mask, the boxes kept; a row in checked.jsonl). Returns how many.
fn cutoff_labels(video: &Path, tracks: &Path, report: &Value, exclude: Vec<[f64; 4]>, offset: f64, out: &Path) -> Result<usize, String> {
    let tracks: Tracks = serde_json::from_slice(&std::fs::read(tracks).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let frame_of = |v: &Value| v.as_f64().map(|f| f as i64);
    let ((start, end), near) = if report["mode"] == "track" {
        ((frame_of(&report["summary"]["start"]), frame_of(&report["summary"]["end"])), 0.0)
    } else {
        let flicks = report["flicks"].as_array().cloned().unwrap_or_default();
        let first = flicks.iter().filter_map(|m| frame_of(&m["start_frame"])).min();
        let last = flicks.iter().filter_map(|m| frame_of(&m["kill_frame"])).max();
        ((first, last), 2.0)
    };
    if start.is_none() {
        return Ok(0);
    }
    let video_name = video.to_string_lossy().into_owned();
    let request = CutoffRequest { frames: tracks.frames, video: video_name, start, end, exclude: Some(exclude), offset, near };
    let crops = cutoff_crops(&request);
    if crops.is_empty() {
        return Ok(0);
    }
    crate::ffmpeg::ensure(|_, _| {})?;
    let info = probe(video)?;
    let fixed = crate::review::fixed_map(video, &info, |_, _| {})?;
    let (mut rows, mut n) = (Vec::new(), 0);
    for crop in &crops {
        let Ok(rgb) = frame_rgb(video, &info, crop.frame) else { continue };
        write_crop(out, crop, &rgb, &fixed)?;
        rows.extend(pyjson::to_vec(&crop.row, false));
        rows.push(b'\n');
        n += 1;
    }
    // a later submit's rows win (hand_crops.py: to_dataset)
    pyjson::append_text(&out.join("checked.jsonl"), &rows)?;
    Ok(n)
}

/// Frame `i` of the recording as RGB at 1280 x 720 (ffmpeg's `scale=1280:720:flags=area,format=rgb24`), decoded from
/// the key frame before it, as the review decodes its runs.
fn frame_rgb(video: &Path, info: &VideoInfo, i: usize) -> Result<Vec<u8>, String> {
    let key = info
        .keys
        .iter()
        .filter_map(|&k| info.times.iter().position(|&t| t == k))
        .filter(|&k| k <= i)
        .max()
        .unwrap_or(0);
    let mut frames = Frames::open(video, (key > 0).then(|| info.times[key]), Some(i - key + 1))?;
    let mut yuv = vec![0u8; crate::review::frame_bytes(info)];
    for _ in key..=i {
        if !frames.next_into(&mut yuv)? {
            return Err(format!("frame {i} could not be decoded"));
        }
    }
    let mut rgb = vec![0u8; W * H * 3];
    Converter::new(info.width, info.height, info.matrix, info.full).rgb24(&yuv, &mut rgb);
    Ok(rgb)
}

/// One crop's file, as `np.savez_compressed(rgb=, fixed=, tmask=, boxes=, hidden=)` in hand_crops.py.
fn write_crop(out: &Path, crop: &CutoffCrop, rgb: &[u8], fixed: &[u8]) -> Result<(), String> {
    let (x0, y0) = (crop.x0, crop.y0);
    let mut pixels = Vec::with_capacity(CROP * CROP * 3);
    let mut fixed_crop = Vec::with_capacity(CROP * CROP);
    for y in y0..y0 + CROP {
        pixels.extend_from_slice(&rgb[(y * W + x0) * 3..(y * W + x0 + CROP) * 3]);
        fixed_crop.extend_from_slice(&fixed[y * W + x0..y * W + x0 + CROP]);
    }
    let boxes: Vec<f32> = crop.boxes.iter().flat_map(|b| b.map(|v| v as f32)).collect();
    let arrays = [
        ("rgb", &Array::u8(&[CROP, CROP, 3], pixels)),
        ("fixed", &Array::u8(&[CROP, CROP], fixed_crop)),
        ("tmask", &Array::u8(&[CROP, CROP], vec![0; CROP * CROP])),
        ("boxes", &Array::f32(&[crop.boxes.len(), 4], &boxes)),
        ("hidden", &Array::u8(&[], vec![0])),
    ];
    npz::save(&out.join(&crop.row.file), &arrays)
}
