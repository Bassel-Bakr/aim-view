//! The faint-target cut-off (python/server.py: faint, set_faint, submit_faint, skip_faint, faint_queue).
//!
//! In: the page's cut-off for a recording ({on, offset}), its submits and its skips. Kept: faint.json in the
//! recording's folder, which a tracking run's report measures with (report.rs), and faint_skipped.json, the recordings
//! left out of the cut-off queue. Out: a submitted cut-off written as detector labels in the layout's cutoff folder
//! (the core picks the crops, python/model/hand_crops.py's `cutoff_crops`; here each crop's pixels and the fixed map
//! are read from the recording, and written as that script writes them).

use std::ops::RangeInclusive;
#[cfg(feature = "native")]
use std::path::Path;
use std::path::PathBuf;

#[cfg(feature = "native")]
use aimview::convert::{Converter, DST_H, DST_W};
use aimview::faint::DEFAULT_OFFSET;
#[cfg(feature = "native")]
use aimview::faint::{CROP, CutoffCrop, CutoffRequest, cutoff_crops};
#[cfg(feature = "native")]
use aimview::track::Tracks;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::library::{Answer, Failure, Library, local_stamp};
#[cfg(feature = "native")]
use crate::npz::{self, Array};
use crate::pyjson;
#[cfg(feature = "native")]
use crate::video::{Frames, VideoInfo, probe};

const FAINT: &str = "faint.json";
const FAINT_SKIPPED: &str = "faint_skipped.json";
/// The review's tracks, in the folder of the review shown.
const TRACKS: &str = "tracks.json";
/// A submit's label rows, in the cutoff folder (its crops go in train/).
#[cfg(feature = "native")]
const CHECKED_ROWS: &str = "checked.jsonl";
/// The offsets a cut-off can have (python/server.py's check).
const OFFSET_RANGE: RangeInclusive<f64> = 0.2..=0.6;
/// The decimals an offset is kept with.
const OFFSET_DECIMALS: usize = 2;
/// How near the crosshair, in degrees, a track's frames do not count toward its score (`CutoffRequest`'s `near`; a
/// target under the crosshair scores low). A tracking run's bot is under the crosshair most of the time, so every
/// frame counts; a clicking run leaves out the targets being shot.
#[cfg(feature = "native")]
const TRACKING_NEAR_DEG: f64 = 0.0;
#[cfg(feature = "native")]
const CLICKING_NEAR_DEG: f64 = 2.0;
/// Bytes per pixel of an RGB frame.
#[cfg(feature = "native")]
const RGB_BYTES: usize = 3;

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

/// Python's truth of a value (`bool(value)`).
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64() != Some(0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(fields) => !fields.is_empty(),
    }
}

/// The time now as Python's `datetime.now().isoformat(timespec="seconds")`.
fn now_iso() -> String {
    // local_stamp writes YYYY.MM.DD-HH.MM.SS
    let stamp = local_stamp(crate::disk::now().floor());
    let (date, time) = stamp.split_once('-').unwrap_or((&stamp, ""));
    format!("{}T{}", date.replace('.', "-"), time.replace('.', ":"))
}

/// A body's offset as python/server.py reads it (`float(body.get("offset", DEFAULT_OFFSET))`), checked to be one the
/// cut-off can have.
fn offset_of(body: &Value) -> Answer<f64> {
    let offset = match body.get("offset") {
        None => DEFAULT_OFFSET,
        Some(Value::Number(number)) => number.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(text)) => {
            text.trim().parse().map_err(|_| Failure::bad(format!("could not convert string to float: '{text}'")))?
        }
        Some(Value::Bool(flag)) => f64::from(u8::from(*flag)),
        Some(other) => return Err(Failure::bad(format!("offset must be a number, not {other}"))),
    };
    if !OFFSET_RANGE.contains(&offset) {
        return Err(Failure::bad("offset must be between 0.2 and 0.6"));
    }
    Ok(offset)
}

/// The submit record a change of the cut-off keeps: a new submit's (`submitted`, its labels not counted yet), else
/// the last one's in `old` (faint.json as kept), else none.
fn kept_record(submitted: Option<String>, old: &Value) -> Option<Submitted> {
    if let Some(at) = submitted {
        return Some(Submitted { submitted: at, labels: Value::Null });
    }
    truthy(&old["submitted"]).then(|| Submitted {
        submitted: old["submitted"].as_str().map_or_else(|| old["submitted"].to_string(), str::to_string),
        labels: old.get("labels").cloned().unwrap_or(Value::Null),
    })
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

    /// Keeps the cut-off ({on, offset}); a later change keeps the record of the last submit. The report measures with
    /// it when it is next shown.
    pub fn set_faint(&self, id: &str, body: &Value, submitted: Option<String>) -> Answer<Value> {
        let on = truthy(&body["on"]);
        let offset = offset_of(body)?;
        let record = kept_record(submitted, &self.faint(id));
        let new = FaintFile { on, offset: aimview::python::round(offset, OFFSET_DECIMALS), record };
        pyjson::dump(&self.faint_path(id), &new, false)?;
        Ok(self.faint(id))
    }

    /// The user's cut-off, submitted: kept (on), and the review's tracks written as detector labels in the background.
    /// In the browser build only kept: the page makes the labels (cutoff.worker.ts) and downloads them.
    pub fn submit_faint(&self, id: &str, offset: f64) -> Answer<Value> {
        let dir = self.shown(id).1;
        let report = match self.report(id) {
            Ok(report) if crate::disk::is_file(dir.join(TRACKS)) && !report.is_null() => report,
            Ok(_) => return Err(Failure::bad("review the recording first")),
            Err(failure) => return Err(failure),
        };
        let out = self.set_faint(id, &json!({ "on": true, "offset": offset }), Some(now_iso()))?;
        #[cfg(feature = "native")]
        self.write_cutoff_labels(id, dir, report, offset)?;
        #[cfg(not(feature = "native"))]
        let _ = (dir, report);
        Ok(out)
    }

    /// A submitted cut-off's labels, written in the background (`cutoff_labels`), and their count kept in faint.json.
    #[cfg(feature = "native")]
    fn write_cutoff_labels(&self, id: &str, dir: PathBuf, report: Value, offset: f64) -> Answer<()> {
        let video: PathBuf = self.resolve(id)?.components().collect();
        let exclude: Vec<[f64; 4]> = self.exclude_areas(id);
        // the labels go where python/ keeps them (the layout's cutoff folder): crops in train/, rows in checked.jsonl
        let (faint, labels) = (self.faint_path(id), self.folders().cutoff.clone());
        std::thread::spawn(move || {
            let label_count = match cutoff_labels(&video, &dir.join(TRACKS), &report, exclude, offset, &labels) {
                Ok(count) => count,
                Err(error) => {
                    eprintln!("the cut-off's labels: {error}");
                    return;
                }
            };
            let kept = pyjson::load(&faint).and_then(|value| serde_json::from_value::<FaintFile>(value).ok());
            let Some(mut file) = kept else { return };
            if let Some(record) = file.record.as_mut() {
                record.labels = json!(label_count);
            }
            if let Err(error) = pyjson::dump(&faint, &file, false) {
                eprintln!("the cut-off's labels: {error}");
            }
        });
        Ok(())
    }

    /// Skipped in the cut-off queue: left out of it from now on.
    pub fn skip_faint(&self, id: &str) -> Answer<Value> {
        crate::labels::add_id(&self.file(FAINT_SKIPPED), id)
    }

    /// Recordings to set a cut-off in, in the area queue's order, leaving out probes, other games, skipped and
    /// submitted ones.
    pub fn faint_queue(&self) -> Answer<Value> {
        let skipped = crate::labels::read_ids(&self.file(FAINT_SKIPPED));
        let submitted = |id: &str| {
            self.faint_file(id).and_then(|file| file.record).is_some_and(|record| !record.submitted.is_empty())
        };
        Ok(json!(self.queue(|id| skipped.contains(id) || submitted(id))?))
    }
}

/// The labels of a submitted cut-off (python/server.py: submit_faint's run): the crops the core picks from the run's
/// frames, each written as hand_crops.py writes them (train/<stem>_<frame>.npz: the crop's RGB and fixed map, an empty
/// target mask, the boxes kept; a row in checked.jsonl). Returns how many.
#[cfg(feature = "native")]
fn cutoff_labels(
    video: &Path,
    tracks: &Path,
    report: &Value,
    exclude: Vec<[f64; 4]>,
    offset: f64,
    out: &Path,
) -> Result<usize, String> {
    let bytes = crate::disk::read(tracks).map_err(|error| error.to_string())?;
    let tracks: Tracks = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    let (start, end, near) = run_span(report);
    if start.is_none() {
        return Ok(0);
    }
    let video_name = video.to_string_lossy().into_owned();
    let request =
        CutoffRequest { frames: tracks.frames, video: video_name, start, end, exclude: Some(exclude), offset, near };
    let crops = cutoff_crops(&request);
    if crops.is_empty() {
        return Ok(0);
    }
    crate::ffmpeg::ensure(|_, _| {})?;
    let info = probe(video)?;
    let fixed = crate::review::fixed_map(video, &info, |_, _| {})?;
    let (mut rows, mut label_count) = (Vec::new(), 0);
    for crop in &crops {
        let Ok(rgb) = frame_rgb(video, &info, crop.frame) else { continue };
        write_crop(out, crop, &rgb, &fixed)?;
        rows.extend(pyjson::to_vec(&crop.row, false));
        rows.push(b'\n');
        label_count += 1;
    }
    // a later submit's rows win (hand_crops.py: to_dataset)
    pyjson::append_text(&out.join(CHECKED_ROWS), &rows)?;
    Ok(label_count)
}

/// The frames a submit's crops come from and how near the crosshair a score does not count, in degrees
/// (`CutoffRequest`'s start, end and near): a tracking run's own first and last frames; a clicking run's first flick's
/// start and its last kill.
#[cfg(feature = "native")]
fn run_span(report: &Value) -> (Option<i64>, Option<i64>, f64) {
    let frame_of = |value: &Value| value.as_f64().map(|frame| frame as i64);
    if report["mode"] == "track" {
        return (frame_of(&report["summary"]["start"]), frame_of(&report["summary"]["end"]), TRACKING_NEAR_DEG);
    }
    let flicks: &[Value] = report["flicks"].as_array().map_or(&[], Vec::as_slice);
    let first = flicks.iter().filter_map(|flick| frame_of(&flick["start_frame"])).min();
    let last = flicks.iter().filter_map(|flick| frame_of(&flick["kill_frame"])).max();
    (first, last, CLICKING_NEAR_DEG)
}

/// A frame of the recording as RGB at 1280 x 720 (ffmpeg's `scale=1280:720:flags=area,format=rgb24`), decoded from
/// the key frame before it, as the review decodes its runs.
#[cfg(feature = "native")]
fn frame_rgb(video: &Path, info: &VideoInfo, frame: usize) -> Result<Vec<u8>, String> {
    let key = info
        .keys
        .iter()
        .filter_map(|&key_time| info.times.iter().position(|&time| time == key_time))
        .filter(|&key| key <= frame)
        .max()
        .unwrap_or(0);
    let mut frames = Frames::open(video, (key > 0).then(|| info.times[key]), Some(frame - key + 1))?;
    let mut yuv = vec![0u8; crate::review::frame_bytes(info)];
    for _ in key..=frame {
        if !frames.next_into(&mut yuv)? {
            return Err(format!("frame {frame} could not be decoded"));
        }
    }
    let mut rgb = vec![0u8; DST_W * DST_H * RGB_BYTES];
    Converter::new(info.width, info.height, info.matrix, info.full).rgb24(&yuv, &mut rgb);
    Ok(rgb)
}

/// One crop's file, as `np.savez_compressed(rgb=, fixed=, tmask=, boxes=, hidden=)` in hand_crops.py.
#[cfg(feature = "native")]
fn write_crop(out: &Path, crop: &CutoffCrop, rgb: &[u8], fixed: &[u8]) -> Result<(), String> {
    let (x0, y0) = (crop.x0, crop.y0);
    let mut pixels = Vec::with_capacity(CROP * CROP * RGB_BYTES);
    let mut fixed_crop = Vec::with_capacity(CROP * CROP);
    for y in y0..y0 + CROP {
        let row_start = y * DST_W + x0;
        pixels.extend_from_slice(&rgb[row_start * RGB_BYTES..(row_start + CROP) * RGB_BYTES]);
        fixed_crop.extend_from_slice(&fixed[row_start..row_start + CROP]);
    }
    let boxes: Vec<f32> = crop.boxes.iter().flat_map(|crop_box| crop_box.map(|value| value as f32)).collect();
    let arrays = [
        ("rgb", &Array::u8(&[CROP, CROP, RGB_BYTES], pixels)),
        ("fixed", &Array::u8(&[CROP, CROP], fixed_crop)),
        ("tmask", &Array::u8(&[CROP, CROP], vec![0; CROP * CROP])),
        ("boxes", &Array::f32(&[crop.boxes.len(), 4], &boxes)),
        ("hidden", &Array::u8(&[], vec![0])),
    ];
    npz::save(&out.join(&crop.row.file), &arrays)
}
