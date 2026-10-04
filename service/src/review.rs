//! A recording's review on this computer: ffmpeg decodes the frames, the core converts them to ffmpeg's 720p RGB byte
//! for byte, the detector runs on the GPU (detector.rs), and the core's review session (aimview::session, which the
//! browser's workers feed the same way) does the rest: it plans the runs, reads the key frames, tracks each run's
//! frames, watches the camera's turn and the HUD, and joins the runs. The runs are reviewed at once: one ffmpeg decoder
//! is the limit, as one browser decoder was. The browser build has only what a review is (`Request`): the page runs it
//! (library/browser.rs). In: a `Request` (library/reviews.rs, aimview-tool, the track example). Out: the review's
//! tracks, readings, HUD reading and found areas (`Reviewed`), which library/reviews.rs keeps.

use std::path::PathBuf;
#[cfg(feature = "native")]
use std::path::Path;
#[cfg(feature = "native")]
use std::sync::atomic::{AtomicUsize, Ordering};
#[cfg(feature = "native")]
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
#[cfg(feature = "native")]
use std::sync::Mutex;
#[cfg(feature = "native")]
use std::thread;

use aimview::areas::Found;
#[cfg(feature = "native")]
use aimview::areas::{AreaFinder, FRAME, sample_frames};
use aimview::camera::VideoReadings;
#[cfg(feature = "native")]
use aimview::convert::{Converter, DST_H, DST_W};
#[cfg(feature = "native")]
use aimview::fixed::FixedMap;
use aimview::hud::HudReading;
#[cfg(feature = "native")]
use aimview::session::{
    FrameFormat, Joined, KeysRead, NextFrame, Review, RunTracking, Setup, WatchPart, countdown_bytes,
};
#[cfg(feature = "native")]
use aimview::tracker::TrackPart;

pub use aimview::session::{AreaBox, TimeWindow, Tracks};

use crate::config::Device;
#[cfg(feature = "native")]
use crate::detector::{Detector, MAP_HEIGHT, MAP_WIDTH, model_settings};
#[cfg(feature = "native")]
use crate::video::{Frames, VideoInfo, probe};

/// Frames between progress reports.
#[cfg(feature = "native")]
const PROGRESS_EVERY: usize = 60;
/// A 720p frame's RGB24 bytes, as the detector takes it.
#[cfg(feature = "native")]
const RGB_BYTES: usize = DST_W * DST_H * 3;
/// The key frames decoded at once (`key_frames`).
#[cfg(feature = "native")]
const KEY_DECODERS: usize = 4;
/// The batches waiting for the detector, and the frames waiting for the watch, before the decoder waits for them.
#[cfg(feature = "native")]
const BATCHES_WAITING: usize = 2;
#[cfg(feature = "native")]
const WATCH_FRAMES_WAITING: usize = 8;

/// What to review: the video, the detector model (its _u8in export) and the device it runs on, the frames it takes at
/// once, the scenario's target count (0: not known), the runs to split the recording into, the part of the video to
/// track (the user's run window with a margin; None: all of it), the areas it leaves out (the recording's, areas.rs),
/// and a folder to keep the review's parts in before they are joined (`keep_parts`; None: not kept).
pub struct Request {
    pub video: PathBuf,
    pub model: PathBuf,
    pub device: Device,
    pub batch: usize,
    pub cap: usize,
    pub runs: usize,
    pub window: Option<TimeWindow>,
    pub areas: Vec<AreaBox>,
    pub keep_parts: Option<PathBuf>,
}

/// A review's tracks, the video's readings, what the HUD read (None: no HUD was read), and the areas the area finder
/// found in the key frames it read (None when the recording has too few for it: areas.rs reads its frames then).
pub struct Reviewed {
    pub tracks: Tracks,
    pub readings: VideoReadings,
    pub hud: Option<HudReading>,
    pub found: Option<Found>,
}

/// Where a review stands: its stage ("looking" at the key frames, "tracking", "linking"), frames done, of how many.
pub type Progress<'a> = &'a (dyn Fn(&str, usize, usize) + Sync);

/// Told the device each run's detector runs on ("DirectML", "CUDA" or "CPU") once it has loaded: with `Device::Auto`
/// the CPU when the GPU could not start it.
pub type DeviceNote<'a> = &'a (dyn Fn(&'static str) + Sync);

/// Adds a device to the devices a review ran on: "DirectML", or "DirectML and CPU" when its runs' differ.
pub fn add_device(devices: &mut String, device: &str) {
    if !devices.split(" and ").any(|named| named == device) {
        if !devices.is_empty() {
            devices.push_str(" and ");
        }
        devices.push_str(device);
    }
}

/// The size of one frame as ffmpeg gives it (YUV 4:2:0 at the video's size: the Y plane, then U and V at half the size
/// each way).
#[cfg(feature = "native")]
pub(crate) fn frame_bytes(info: &VideoInfo) -> usize {
    info.width * info.height + 2 * info.width.div_ceil(2) * info.height.div_ceil(2)
}

/// Reviews a recording: its tracks and readings. `on_device` is told where each run's detector runs.
#[cfg(feature = "native")]
pub fn review(req: &Request, progress: Progress, on_device: DeviceNote) -> Result<Reviewed, String> {
    let model = model_settings(&req.model)?;
    let info = probe(&req.video)?;
    let format = FrameFormat { width: info.width, height: info.height, matrix: info.matrix.code(), full: info.full };
    let review = Review::new(Setup {
        fps: info.fps,
        times: info.times.clone(),
        keys: info.keys.clone(),
        format,
        cap: req.cap,
        areas: req.areas.clone(),
        window: req.window,
        runs: req.runs,
        model,
    })?;
    let total = review.frames();
    progress("looking", 0, total);
    let (keys, finder) = read_keys(req, &review, &info)?;
    let session = keys.hud.session();
    let finding = thread::spawn(move || finder.map(|finder| finder.finish(session)));
    let done = AtomicUsize::new(0);
    let context = RunContext { req, review: &review, keys: &keys, info: &info, done: &done, progress, on_device };
    let parts: Vec<Result<RunPart, String>> = thread::scope(|scope| {
        let context = &context;
        let running: Vec<_> =
            (0..review.runs().len()).map(|run| scope.spawn(move || review_run(context, run))).collect();
        let failed = || Err("a run of the review failed".into());
        running.into_iter().map(|running| running.join().unwrap_or_else(|_| failed())).collect()
    });
    progress("linking", total, total);
    let joined = join_runs(req, &review, &keys, parts)?;
    let found = finding.join().map_err(|_| "the area finder failed")?;
    Ok(Reviewed { tracks: joined.tracks, readings: joined.readings, hud: joined.hud, found })
}

/// Reads the key frames for the fixed map and the HUD's boxes, and for the area finder when the recording has enough
/// of them (python/areas.py: sample; None otherwise: areas.rs reads its frames then).
#[cfg(feature = "native")]
fn read_keys(req: &Request, review: &Review, info: &VideoInfo) -> Result<(KeysRead, Option<AreaFinder>), String> {
    let mut keys = review.keys();
    let mut finder = sample_frames(info.keys.len(), &info.times, info.duration).is_none().then(AreaFinder::new);
    key_frames(&req.video, info, |small, luma| {
        let contrast = aimview::fixed::contrast(small);
        keys.add_contrast(&contrast, luma);
        if let Some(finder) = finder.as_mut() {
            finder.add_contrast(small, &contrast);
        }
    })?;
    Ok((keys.finish(), finder))
}

/// The runs' parts joined, each kept first when the request asks (`keep_parts`), with the devices their detectors ran
/// on.
#[cfg(feature = "native")]
fn join_runs(
    req: &Request,
    review: &Review,
    keys: &KeysRead,
    parts: Vec<Result<RunPart, String>>,
) -> Result<Joined, String> {
    let mut joining = review.joining(&keys.fixed);
    let mut devices = String::new();
    for (run, part) in parts.into_iter().enumerate() {
        let part = part?;
        if let Some(dir) = &req.keep_parts {
            keep_part(dir, &format!("run{run}_track.json"), &as_json(&part.track)?)?;
            keep_part(dir, &format!("run{run}_watch.json"), &as_json(&part.watch)?)?;
        }
        joining.add(part.track, part.watch);
        add_device(&mut devices, part.device);
    }
    let detector = format!("onnxruntime ({devices})");
    if let Some(dir) = &req.keep_parts {
        keep_part(dir, "setup.json", &as_json(review.setup())?)?;
        keep_part(dir, "fixed.bin", &keys.fixed)?;
        keep_part(dir, "detector.txt", detector.as_bytes())?;
    }
    joining.finish(detector)
}

/// One file of a review's parts, kept before they are joined (`Request::keep_parts`): what the stages after the
/// detector need to make the review again without the video (tests/replay.rs).
#[cfg(feature = "native")]
fn keep_part(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), String> {
    std::fs::create_dir_all(dir)
        .and_then(|()| std::fs::write(dir.join(name), bytes))
        .map_err(|error| format!("{}: {error}", dir.join(name).display()))
}

/// A part as JSON.
#[cfg(feature = "native")]
fn as_json(value: &impl serde::Serialize) -> Result<Vec<u8>, String> {
    serde_json::to_vec(value).map_err(|error| error.to_string())
}

/// A key frame: at 720p for the fixed map (FRAME bytes, YUV 4:2:0), and as decoded (the HUD reads its Y plane).
#[cfg(feature = "native")]
type KeyFrame = (Vec<u8>, Vec<u8>);

/// The key frame at `time` (seconds), decoded on its own and converted to 720p with `convert`.
#[cfg(feature = "native")]
fn decode_key_frame(video: &Path, info: &VideoInfo, convert: &mut Converter, time: f64) -> Result<KeyFrame, String> {
    let mut yuv = vec![0u8; frame_bytes(info)];
    let mut frames = Frames::open(video, (time > info.times[0]).then_some(time), Some(1))?;
    if !frames.next_into(&mut yuv)? {
        return Err(format!("the key frame at {time:.3} s could not be decoded"));
    }
    let mut small = vec![0u8; FRAME];
    convert.yuv420p(&yuv, &mut small);
    Ok((small, yuv))
}

/// Each KEY_DECODERS-th key frame from the `first`-th, decoded and sent; it stops at the first that fails, or once
/// nothing takes them.
#[cfg(feature = "native")]
fn decode_key_frames(video: &Path, info: &VideoInfo, first: usize, sender: &SyncSender<Result<KeyFrame, String>>) {
    let mut convert = Converter::new(info.width, info.height, info.matrix, info.full);
    for &time in info.keys.iter().skip(first).step_by(KEY_DECODERS) {
        let frame = decode_key_frame(video, info, &mut convert, time);
        let failed = frame.is_err();
        if sender.send(frame).is_err() || failed {
            break;
        }
    }
}

/// Each key frame handed to `key`, in order: at 1280 x 720 (YUV 4:2:0), and its Y plane at the video's size. Each key
/// frame is decoded on its own, from its exact time (ffmpeg's libaom ignores `-skip_frame nokey` and gives every
/// frame), a few at once: decoder i takes every KEY_DECODERS-th key frame from the i-th and waits while its next one
/// is not wanted yet, so only a few whole frames are held at a time.
#[cfg(feature = "native")]
pub(crate) fn key_frames(video: &Path, info: &VideoInfo, mut key: impl FnMut(&[u8], &[u8])) -> Result<(), String> {
    thread::scope(|scope| {
        let (decoders, decoded): (Vec<_>, Vec<_>) = (0..KEY_DECODERS.min(info.keys.len()))
            .map(|first| {
                let (sender, decoded) = mpsc::sync_channel::<Result<KeyFrame, String>>(1);
                (scope.spawn(move || decode_key_frames(video, info, first, &sender)), decoded)
            })
            .unzip();
        let taken = (|| {
            for i in 0..info.keys.len() {
                let received = decoded[i % KEY_DECODERS].recv().map_err(|_| "a key frame failed".to_string())?;
                let (small, yuv) = received?;
                key(&small, &yuv[..info.width * info.height]);
            }
            Ok(())
        })();
        // the decoders stop once nothing takes their frames
        drop(decoded);
        for decoder in decoders {
            let _ = decoder.join();
        }
        taken
    })
}

/// The fixed map from the key frames (as python/retired/review.py builds it), each key frame also handed to `key` as
/// `key_frames` does.
#[cfg(feature = "native")]
pub(crate) fn fixed_map(video: &Path, info: &VideoInfo, mut key: impl FnMut(&[u8], &[u8])) -> Result<Vec<u8>, String> {
    let mut map = FixedMap::default();
    key_frames(video, info, |small, luma| {
        map.add(small);
        key(small, luma);
    })?;
    Ok(map.map())
}

/// A run's part of the review: its tracking's and its watches' parts, and where its detector ran.
#[cfg(feature = "native")]
struct RunPart {
    track: TrackPart,
    watch: WatchPart,
    device: &'static str,
}

/// What every run of a review shares: the request, the review session, the key frames read, the video, the frames
/// tracked so far in all runs (for progress), and who is told the progress and the devices.
#[cfg(feature = "native")]
struct RunContext<'a> {
    req: &'a Request,
    review: &'a Review,
    keys: &'a KeysRead,
    info: &'a VideoInfo,
    done: &'a AtomicUsize,
    progress: Progress<'a>,
    on_device: DeviceNote<'a>,
}

/// A batch of frames for the detector: their RGB bytes, and how many of them are the run's (the rest of the last batch
/// is left over).
#[cfg(feature = "native")]
type Batch = (Vec<u8>, usize);

/// A frame for the watch: its Y plane (at the video's size) and its countdown rows.
#[cfg(feature = "native")]
type WatchFrame = (Vec<u8>, Vec<u8>);

/// A channel to one of a run's threads, and the buffers that thread sends back to be filled again.
#[cfg(feature = "native")]
struct Handoff<T> {
    send: SyncSender<T>,
    spare: Receiver<Vec<u8>>,
}

/// One run: this thread decodes and converts each frame the run reads and watches the excluded areas of those it
/// tracks (`decode_run`); a detector thread takes those frames a batch at a time and hands their maps to the tracking
/// in order (`detect`); a watch thread reads the camera's turn and the HUD from each frame's Y plane and countdown rows
/// (`watch_run`).
#[cfg(feature = "native")]
fn review_run(context: &RunContext, run: usize) -> Result<RunPart, String> {
    let RunContext { req, review, keys, info, .. } = *context;
    let planned = &review.runs()[run];
    let batch = req.batch.max(1);
    let mut frames = Frames::open(&req.video, (planned.first > 0).then_some(planned.from), Some(planned.reads()))?;
    let detector = Detector::new(&req.model, batch, &keys.fixed, req.device)?;
    let device = detector.device;
    (context.on_device)(device);
    let tracking = Mutex::new(review.tracking(run));
    let (to_detector, batches) = mpsc::sync_channel::<Batch>(BATCHES_WAITING);
    let (spare_rgb, spare_rgb_back) = mpsc::channel::<Vec<u8>>();
    let (to_watch, watch_frames) = mpsc::sync_channel::<WatchFrame>(WATCH_FRAMES_WAITING);
    let (spare_luma, spare_luma_back) = mpsc::channel::<Vec<u8>>();
    let watch = thread::scope(|scope| {
        let tracking = &tracking;
        let detecting = scope.spawn(move || detect(context, detector, batches, &spare_rgb, tracking));
        let watching = scope.spawn(move || watch_run(review, run, keys, watch_frames, &spare_luma));
        let to_detector = Handoff { send: to_detector, spare: spare_rgb_back };
        let to_watch = Handoff { send: to_watch, spare: spare_luma_back };
        let decoded = decode_run(&mut frames, info, batch, tracking, &to_detector, &to_watch);
        drop(to_detector);
        drop(to_watch);
        let detected = detecting.join().unwrap_or_else(|_| Err("the detector failed".into()));
        let watched = watching.join().map_err(|_| "the camera watch failed")?;
        detected?;
        decoded?;
        watched
    })?;
    let track = tracking.into_inner().map_err(|_| "the tracker failed")?.part()?;
    Ok(RunPart { track, watch, device })
}

/// The decoder's side of a run: each frame decoded and converted; its Y plane and countdown rows go to the watch, and
/// each frame the run tracks has its excluded areas watched and goes to the detector, `batch` frames at a time (the
/// last batch with the frames it has). It stops where the tracking says the run ends.
#[cfg(feature = "native")]
fn decode_run(
    frames: &mut Frames,
    info: &VideoInfo,
    batch: usize,
    tracking: &Mutex<RunTracking>,
    to_detector: &Handoff<Batch>,
    to_watch: &Handoff<WatchFrame>,
) -> Result<(), String> {
    let stopped = || "the detector stopped".to_string();
    let rows = countdown_bytes();
    let luma_bytes = info.width * info.height;
    let mut convert = Converter::new(info.width, info.height, info.matrix, info.full);
    let mut yuv = vec![0u8; frame_bytes(info)];
    let mut rgb = vec![0u8; RGB_BYTES];
    let mut waiting = vec![0u8; batch * RGB_BYTES];
    let mut count = 0;
    while frames.next_into(&mut yuv)? {
        let next = tracking.lock().map_err(|_| "the tracker failed")?.next_frame();
        if next == NextFrame::Stop {
            break;
        }
        convert.rgb24(&yuv, &mut rgb);
        let mut luma = to_watch.spare.try_recv().unwrap_or_else(|_| vec![0u8; luma_bytes]);
        luma.copy_from_slice(&yuv[..luma_bytes]);
        to_watch.send.send((luma, rgb[rows.clone()].to_vec())).map_err(|_| "the camera watch stopped")?;
        if next == NextFrame::Watch {
            continue;
        }
        tracking.lock().map_err(|_| "the tracker failed")?.watch(&rgb);
        waiting[count * RGB_BYTES..(count + 1) * RGB_BYTES].copy_from_slice(&rgb);
        count += 1;
        if count == batch {
            let next = to_detector.spare.try_recv().unwrap_or_else(|_| vec![0u8; batch * RGB_BYTES]);
            to_detector.send.send((std::mem::replace(&mut waiting, next), count)).map_err(|_| stopped())?;
            count = 0;
        }
    }
    if count > 0 {
        to_detector.send.send((waiting, count)).map_err(|_| stopped())?;
    }
    Ok(())
}

/// The detector's side of a run: each batch run through the detector, its frames' maps handed to the tracking in
/// order, and its buffer sent back to be filled again; progress is told every PROGRESS_EVERY frames of all the runs.
#[cfg(feature = "native")]
fn detect(
    context: &RunContext,
    mut detector: Detector,
    batches: Receiver<Batch>,
    spare: &Sender<Vec<u8>>,
    tracking: &Mutex<RunTracking>,
) -> Result<(), String> {
    let total = context.review.frames();
    for (rgb, count) in batches {
        let maps = detector.run(&rgb)?;
        let _ = spare.send(rgb);
        let mut tracker = tracking.lock().map_err(|_| "the tracker failed")?;
        for index in 0..count {
            let (score, reg) = maps.of_frame(index);
            tracker.maps(score, reg, MAP_WIDTH, MAP_HEIGHT);
        }
        drop(tracker);
        let done = context.done.fetch_add(count, Ordering::Relaxed) + count;
        if done / PROGRESS_EVERY != (done - count) / PROGRESS_EVERY {
            (context.progress)("tracking", done, total);
        }
    }
    Ok(())
}

/// The watch's side of a run: the camera's turn and the HUD read from each frame's Y plane and countdown rows, each
/// Y plane's buffer sent back to be filled again.
#[cfg(feature = "native")]
fn watch_run(
    review: &Review,
    run: usize,
    keys: &KeysRead,
    frames: Receiver<WatchFrame>,
    spare: &Sender<Vec<u8>>,
) -> Result<WatchPart, String> {
    let mut watching = review.watching(run, keys);
    for (luma, rows) in frames {
        watching.frame(&luma, &rows);
        let _ = spare.send(luma);
    }
    watching.part()
}
