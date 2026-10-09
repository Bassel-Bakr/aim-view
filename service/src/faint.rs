//! The faint-target cut-off (python/retired/server.py: faint, set_faint, submit_faint, skip_faint, faint_queue).
//!
//! In: the page's cut-off for a recording ({on, offset}), its submits and its skips. Kept (store.rs): the recording's
//! cut-off (faint.json), which a tracking run's report measures with (report.rs), and the recordings left out of the
//! cut-off queue (faint_skipped.json). Out: a submitted cut-off kept as detector labels (in the layout's cutoff folder:
//! the core picks the crops, python/model/hand_crops.py's `cutoff_crops`; here each crop's pixels and the fixed map are
//! read from the recording, and kept as that script writes them).

use std::collections::BTreeSet;
use std::io::Write;
use std::ops::RangeInclusive;
#[cfg(feature = "native")]
use std::path::{Path, PathBuf};

#[cfg(feature = "native")]
use aimview::convert::{Converter, DST_H, DST_W};
use aimview::faint::DEFAULT_OFFSET;
#[cfg(feature = "native")]
use aimview::faint::{CROP, CutoffCrop, CutoffRequest, cutoff_crops};
#[cfg(feature = "native")]
use aimview::track::Tracks;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use zip::write::SimpleFileOptions;

use crate::library::{Answer, Failure, Library, local_stamp};
#[cfg(feature = "native")]
use crate::npz::{self, Array};
use crate::pyjson;
use crate::store::{IdList, Item, Mark, Part};
#[cfg(feature = "native")]
use crate::store::{ReviewBy, Store};
#[cfg(feature = "native")]
use crate::video::{Frames, VideoInfo, probe};
/// The offsets a cut-off can have (python/retired/server.py's check).
const OFFSET_RANGE: RangeInclusive<f64> = 0.2..=0.6;
/// The decimals an offset is kept with.
const OFFSET_DECIMALS: usize = 2;
/// How near the crosshair, in degrees, a track's frames do not count toward its score in a tracking run
/// (`CutoffRequest`'s `near`; a target under the crosshair scores low). A tracking run's bot is under the crosshair
/// most of the time, so every frame counts.
#[cfg(feature = "native")]
const TRACKING_NEAR_DEG: f64 = 0.0;
/// The same in a clicking run, which leaves out the targets being shot.
#[cfg(feature = "native")]
const CLICKING_NEAR_DEG: f64 = 2.0;
/// Bytes per pixel of an RGB frame.
#[cfg(feature = "native")]
const RGB_BYTES: usize = 3;

/// faint.json as the review server writes it: on and offset, and once submitted when and how many labels it gave.
#[derive(Serialize, Deserialize)]
struct FaintFile {
    /// Whether the report leaves out the tracks the cut-off cuts.
    on: bool,
    /// The cut-off's offset (0.2 to 0.6, two decimals): how far below the recording's level a track scores to be cut.
    offset: f64,
    /// The last submit's record; None: never submitted.
    #[serde(flatten)]
    record: Option<Submitted>,
}

/// A submit's record in faint.json.
#[derive(Serialize, Deserialize)]
struct Submitted {
    /// When it was submitted, as Python's isoformat ("2026-10-07T12:00:00").
    submitted: String,
    /// How many labels it gave; null until they are written.
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

/// A body's offset as python/retired/server.py read it (`float(body.get("offset", DEFAULT_OFFSET))`), checked to be
/// one the cut-off can have.
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
    /// The recording's cut-off as kept ({on, offset}, and submitted and labels once submitted); off by default.
    pub fn faint(&self, id: &str) -> Value {
        let kept = pyjson::load(self.store(), Item::Mark(id, Mark::Cutoff));
        kept.unwrap_or_else(|| json!({ "on": false, "offset": DEFAULT_OFFSET }))
    }

    /// The recording's faint.json read; None when it is missing or not that file.
    fn faint_file(&self, id: &str) -> Option<FaintFile> {
        serde_json::from_value(pyjson::load(self.store(), Item::Mark(id, Mark::Cutoff))?).ok()
    }

    /// Keeps the cut-off ({on, offset}); a later change keeps the record of the last submit. The report measures with
    /// it when it is next shown.
    pub fn set_faint(&self, id: &str, body: &Value, submitted: Option<String>) -> Answer<Value> {
        let on = truthy(&body["on"]);
        let offset = offset_of(body)?;
        let record = kept_record(submitted, &self.faint(id));
        let new = FaintFile { on, offset: aimview::python::round(offset, OFFSET_DECIMALS), record };
        pyjson::dump(self.store(), Item::Mark(id, Mark::Cutoff), &new, false)?;
        Ok(self.faint(id))
    }

    /// The rows of the kept cut-off labels (checked.jsonl), each line's JSON; a line that does not read is left out.
    fn cutoff_rows(&self) -> Vec<Value> {
        let text = self.store().read(Item::CutoffRows).ok().flatten().unwrap_or_default();
        text.split(|&byte| byte == b'\n').filter_map(|line| pyjson::parse(line.trim_ascii()).ok()).collect()
    }

    /// GET /api/cutoff_labels?count=1: how many crops the kept labels have (by file) and from how many recordings.
    pub fn cutoff_labels_count(&self) -> Answer<Value> {
        let rows = self.cutoff_rows();
        let distinct = |key: &str| rows.iter().filter_map(|row| row[key].as_str()).collect::<BTreeSet<_>>().len();
        Ok(json!({ "crops": distinct("file"), "recordings": distinct("video") }))
    }

    /// GET /api/cutoff_labels: the kept labels as the zip detector training reads, as the review server keeps them in
    /// vod_model/hand/cutoff/: checked.jsonl (deflated) and each row's crop, train/<name>.npz (stored: an .npz is
    /// compressed already). A crop that is not kept is left out.
    pub fn cutoff_labels_zip(&self) -> Answer<Vec<u8>> {
        let rows = self.store().read(Item::CutoffRows)?.unwrap_or_default();
        let files: BTreeSet<String> =
            self.cutoff_rows().iter().filter_map(|row| row["file"].as_str().map(str::to_string)).collect();
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let deflated = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let zip_error = |error: zip::result::ZipError| error.to_string();
        zip.start_file(Item::CutoffRows.file_name(), deflated).map_err(zip_error)?;
        zip.write_all(&rows)?;
        for file in &files {
            let Some(npz) = self.store().read(Item::CutoffCrop(file)).ok().flatten() else { continue };
            zip.start_file(file.as_str(), stored).map_err(zip_error)?;
            zip.write_all(&npz)?;
        }
        Ok(zip.finish().map_err(zip_error)?.into_inner())
    }

    /// The user's cut-off, submitted: kept (on), and the review's tracks written as detector labels in the background.
    /// In the browser build only kept: the page makes the labels (cutoff.worker.ts) and sends them
    /// (/api/cutoff_labels).
    pub fn submit_faint(&self, id: &str, offset: f64) -> Answer<Value> {
        let by = self.shown(id).1;
        let report = match self.report(id) {
            Ok(report) if self.store().has(Item::ReviewPart(id, &by, Part::Tracks)) && !report.is_null() => report,
            Ok(_) => return Err(Failure::bad("review the recording first")),
            Err(failure) => return Err(failure),
        };
        let out = self.set_faint(id, &json!({ "on": true, "offset": offset }), Some(now_iso()))?;
        #[cfg(feature = "native")]
        self.write_cutoff_labels(id, by, report, offset)?;
        #[cfg(not(feature = "native"))]
        let _ = (by, report);
        Ok(out)
    }

    /// A submitted cut-off's labels, written in the background (`cutoff_labels`), and their count kept with the
    /// cut-off.
    #[cfg(feature = "native")]
    fn write_cutoff_labels(&self, id: &str, by: ReviewBy, report: Value, offset: f64) -> Answer<()> {
        let video: PathBuf = self.resolve(id)?.components().collect();
        let exclude: Vec<[f64; 4]> = self.exclude_areas(id);
        let (store, id) = (self.shared_store(), id.to_string());
        std::thread::spawn(move || {
            let tracks = store.read(Item::ReviewPart(&id, &by, Part::Tracks));
            let labelled = tracks.map_err(|error| error.to_string()).and_then(|tracks| {
                let tracks = tracks.ok_or("the review has no tracks")?;
                cutoff_labels(&video, &tracks, &report, exclude, offset, &*store)
            });
            let label_count = match labelled {
                Ok(count) => count,
                Err(error) => {
                    eprintln!("the cut-off's labels: {error}");
                    return;
                }
            };
            let faint = Item::Mark(&id, Mark::Cutoff);
            let kept = pyjson::load(&*store, faint).and_then(|value| serde_json::from_value::<FaintFile>(value).ok());
            let Some(mut file) = kept else { return };
            if let Some(record) = file.record.as_mut() {
                record.labels = json!(label_count);
            }
            if let Err(error) = pyjson::dump(&*store, faint, &file, false) {
                eprintln!("the cut-off's labels: {error}");
            }
        });
        Ok(())
    }

    /// Skipped in the cut-off queue: left out of it from now on.
    pub fn skip_faint(&self, id: &str) -> Answer<Value> {
        crate::labels::add_id(self.store(), IdList::FaintSkipped, id)
    }

    /// Recordings to set a cut-off in, in the area queue's order, leaving out probes, other games, skipped and
    /// submitted ones.
    pub fn faint_queue(&self) -> Answer<Value> {
        let skipped = crate::labels::read_ids(self.store(), IdList::FaintSkipped);
        let submitted = |id: &str| {
            self.faint_file(id).and_then(|file| file.record).is_some_and(|record| !record.submitted.is_empty())
        };
        Ok(json!(self.queue(|id| skipped.contains(id) || submitted(id))?))
    }
}

/// The labels of a submitted cut-off (python/retired/server.py: submit_faint's run): the crops the core picks from the
/// run's frames (the review's `tracks`), each kept as hand_crops.py writes them (train/<stem>_<frame>.npz: the crop's
/// RGB and fixed map, an empty target mask, the boxes kept; a row in checked.jsonl). Returns how many.
#[cfg(feature = "native")]
fn cutoff_labels(
    video: &Path,
    tracks: &[u8],
    report: &Value,
    exclude: Vec<[f64; 4]>,
    offset: f64,
    store: &dyn Store,
) -> Result<usize, String> {
    let tracks: Tracks = serde_json::from_slice(tracks).map_err(|error| error.to_string())?;
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
        write_crop(store, crop, &rgb, &fixed)?;
        rows.extend(pyjson::to_vec(&crop.row, false));
        rows.push(b'\n');
        label_count += 1;
    }
    // a later submit's rows win (hand_crops.py: to_dataset)
    pyjson::append_text(store, Item::CutoffRows, &rows)?;
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
fn write_crop(store: &dyn Store, crop: &CutoffCrop, rgb: &[u8], fixed: &[u8]) -> Result<(), String> {
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
    let item = Item::CutoffCrop(&crop.row.file);
    store.write(item, &npz::to_bytes(&arrays)?).map_err(|error| format!("{}: {error}", store.name(item)))
}

/// The kept cut-off labels, counted and zipped.
#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;
    use crate::config::{Config, Layout};

    /// Two submits' rows (one crop written twice): counted by crop and recording, and zipped as checked.jsonl and each
    /// kept crop, a crop not kept left out.
    #[test]
    fn labels_are_counted_and_zipped() {
        let dir = std::env::temp_dir().join(format!("aimview-cutoff-labels-{}", std::process::id()));
        let library = Library::open(Config::new(dir.clone(), Layout::App, dir.join("models"))).unwrap();
        let empty = library.cutoff_labels_count().unwrap();
        assert_eq!(empty, json!({ "crops": 0, "recordings": 0 }));
        let rows =
            b"{\"file\": \"train/a.npz\", \"video\": \"x.mp4\"}\n{\"file\": \"train/b.npz\", \"video\": \"x.mp4\"}\n";
        pyjson::append_text(library.store(), Item::CutoffRows, rows).unwrap();
        pyjson::append_text(library.store(), Item::CutoffRows, b"{\"file\": \"train/a.npz\", \"video\": \"y.mp4\"}\n")
            .unwrap();
        library.store().write(Item::CutoffCrop("train/a.npz"), b"npz bytes").unwrap();
        assert_eq!(library.cutoff_labels_count().unwrap(), json!({ "crops": 2, "recordings": 2 }));
        let bytes = library.cutoff_labels_zip().unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let names: Vec<String> = zip.file_names().map(str::to_string).collect();
        assert_eq!(names, ["checked.jsonl", "train/a.npz"]);
        let mut crop = Vec::new();
        zip.by_name("train/a.npz").unwrap().read_to_end(&mut crop).unwrap();
        assert_eq!(crop, b"npz bytes");
        drop(library);
        let _ = std::fs::remove_dir_all(dir);
    }
}
