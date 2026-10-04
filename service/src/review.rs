//! A recording's review on this computer: ffmpeg decodes the frames, the core converts them to ffmpeg's 720p RGB byte
//! for byte, the detector runs on the GPU (detector.rs), and the core's review session (aimview::session, which the
//! browser's workers feed the same way) does the rest: it plans the runs, reads the key frames, tracks each run's
//! frames, watches the camera's turn and the HUD, and joins the runs. The runs are reviewed at once: one ffmpeg decoder
//! is the limit, as one browser decoder was. The browser build has only what a review is (`Request`): the page runs it
//! (library/browser.rs).

use std::path::PathBuf;
#[cfg(feature = "native")]
use std::path::Path;
#[cfg(feature = "native")]
use std::sync::atomic::{AtomicUsize, Ordering};
#[cfg(feature = "native")]
use std::sync::{Mutex, mpsc};
#[cfg(feature = "native")]
use std::thread;

use aimview::areas::Found;
#[cfg(feature = "native")]
use aimview::areas::{AreaFinder, sample_frames};
use aimview::camera::VideoReadings;
#[cfg(feature = "native")]
use aimview::convert::{Converter, DST_H as H, DST_W as W};
#[cfg(feature = "native")]
use aimview::fixed::FixedMap;
use aimview::hud::HudReading;
#[cfg(feature = "native")]
use aimview::session::{FrameFormat, KeysRead, NextFrame, Review, Setup, WatchPart, countdown_bytes};
#[cfg(feature = "native")]
use aimview::tracker::TrackPart;

pub use aimview::session::{AreaBox, TimeWindow, Tracks};

use crate::config::Device;
#[cfg(feature = "native")]
use crate::detector::{Detector, model_settings};
#[cfg(feature = "native")]
use crate::video::{Frames, VideoInfo, probe};

/// Frames between progress reports.
#[cfg(feature = "native")]
const PROGRESS_EVERY: usize = 60;

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
    if !devices.split(" and ").any(|d| d == device) {
        if !devices.is_empty() {
            devices.push_str(" and ");
        }
        devices.push_str(device);
    }
}

/// The size of one frame as ffmpeg gives it (YUV 4:2:0 at the video's size).
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
    let mut keys = review.keys();
    // the area finder reads the same key frames, when there are enough of them (python/areas.py: sample)
    let mut finder = sample_frames(info.keys.len(), &info.times, info.duration).is_none().then(AreaFinder::new);
    key_frames(&req.video, &info, |small, y| {
        let contrast = aimview::fixed::contrast(small);
        keys.add_contrast(&contrast, y);
        if let Some(f) = finder.as_mut() {
            f.add_contrast(small, &contrast);
        }
    })?;
    let keys = keys.finish();
    let session = keys.hud.session();
    let finding = thread::spawn(move || finder.map(|f| f.finish(session)));
    let done = AtomicUsize::new(0);
    let parts: Vec<Result<RunPart, String>> = thread::scope(|s| {
        let running: Vec<_> = (0..review.runs().len())
            .map(|run| {
                let (review, keys, info, done) = (&review, &keys, &info, &done);
                s.spawn(move || review_run(req, review, keys, info, run, done, progress, on_device))
            })
            .collect();
        running.into_iter().map(|r| r.join().unwrap_or_else(|_| Err("a run of the review failed".into()))).collect()
    });
    progress("linking", total, total);
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
    let joined = joining.finish(detector)?;
    let found = finding.join().map_err(|_| "the area finder failed")?;
    Ok(Reviewed { tracks: joined.tracks, readings: joined.readings, hud: joined.hud, found })
}

/// One file of a review's parts, kept before they are joined (`Request::keep_parts`): what the stages after the
/// detector need to make the review again without the video (tests/replay.rs).
#[cfg(feature = "native")]
fn keep_part(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), String> {
    std::fs::create_dir_all(dir)
        .and_then(|()| std::fs::write(dir.join(name), bytes))
        .map_err(|e| format!("{}: {e}", dir.join(name).display()))
}

/// A part as JSON.
#[cfg(feature = "native")]
fn as_json(value: &impl serde::Serialize) -> Result<Vec<u8>, String> {
    serde_json::to_vec(value).map_err(|e| e.to_string())
}

/// A key frame: at 720p for the fixed map, and as decoded (the HUD reads its Y plane).
#[cfg(feature = "native")]
type KeyFrame = (Vec<u8>, Vec<u8>);

/// Each key frame handed to `key`, in order: at 1280 x 720 (YUV 4:2:0), and its Y plane at the video's size. Each key
/// frame is decoded on its own, from its exact time (ffmpeg's libaom ignores `-skip_frame nokey` and gives every
/// frame), a few at once: decoder i takes every AT_ONCE-th key frame from the i-th and waits while its next one is not
/// wanted yet, so only a few whole frames are held at a time.
#[cfg(feature = "native")]
pub(crate) fn key_frames(video: &Path, info: &VideoInfo, mut key: impl FnMut(&[u8], &[u8])) -> Result<(), String> {
    const AT_ONCE: usize = 4;
    thread::scope(|s| {
        let (decoders, decoded): (Vec<_>, Vec<_>) = (0..AT_ONCE.min(info.keys.len()))
            .map(|first| {
                let (sender, decoded) = mpsc::sync_channel::<Result<KeyFrame, String>>(1);
                let decoder = s.spawn(move || {
                    let mut convert = Converter::new(info.width, info.height, info.matrix, info.full);
                    for &t in info.keys.iter().skip(first).step_by(AT_ONCE) {
                        let frame = (|| {
                            let mut yuv = vec![0u8; frame_bytes(info)];
                            let mut frames = Frames::open(video, (t > info.times[0]).then_some(t), Some(1))?;
                            if !frames.next_into(&mut yuv)? {
                                return Err(format!("the key frame at {t:.3} s could not be decoded"));
                            }
                            let mut small = vec![0u8; W * H * 3 / 2];
                            convert.yuv420p(&yuv, &mut small);
                            Ok((small, yuv))
                        })();
                        let failed = frame.is_err();
                        if sender.send(frame).is_err() || failed {
                            break;
                        }
                    }
                });
                (decoder, decoded)
            })
            .unzip();
        let taken = (|| {
            for i in 0..info.keys.len() {
                let (small, yuv) = decoded[i % AT_ONCE].recv().map_err(|_| "a key frame failed".to_string())??;
                key(&small, &yuv[..info.width * info.height]);
            }
            Ok(())
        })();
        // the decoders stop once nothing takes their frames
        drop(decoded);
        for d in decoders {
            let _ = d.join();
        }
        taken
    })
}

/// The fixed map from the key frames (as python/retired/review.py builds it), each key frame also handed to `key` as
/// `key_frames` does.
#[cfg(feature = "native")]
pub(crate) fn fixed_map(video: &Path, info: &VideoInfo, mut key: impl FnMut(&[u8], &[u8])) -> Result<Vec<u8>, String> {
    let mut map = FixedMap::default();
    key_frames(video, info, |small, y| {
        map.add(small);
        key(small, y);
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

/// One run: this thread decodes and converts each frame the run reads and watches the excluded areas of those it
/// tracks; a detector thread takes those frames a batch at a time and hands their maps to the tracking in order; a
/// watch thread reads the camera's turn and the HUD from each frame's Y plane and countdown rows.
#[allow(clippy::too_many_arguments)]
#[cfg(feature = "native")]
fn review_run(
    req: &Request,
    review: &Review,
    keys: &KeysRead,
    info: &VideoInfo,
    run: usize,
    done: &AtomicUsize,
    progress: Progress,
    on_device: DeviceNote,
) -> Result<RunPart, String> {
    let r = &review.runs()[run];
    let total = review.frames();
    let rgb_bytes = W * H * 3;
    let batch = req.batch.max(1);
    let mut frames = Frames::open(&req.video, (r.first > 0).then_some(r.from), Some(r.reads()))?;
    let mut detector = Detector::new(&req.model, batch, &keys.fixed, req.device)?;
    let device = detector.device;
    on_device(device);
    let tracking = Mutex::new(review.tracking(run));
    let rows = countdown_bytes();
    // a batch's frames, and how many of them are the run's (the rest of the last batch is left over)
    let (to_detector, batches) = mpsc::sync_channel::<(Vec<u8>, usize)>(2);
    let (spare_tx, spare) = mpsc::channel::<Vec<u8>>();
    // a frame's Y plane (at the video's size) and its countdown rows; the Y planes come back to be filled again
    let y_bytes = info.width * info.height;
    let (to_watch, watch_frames) = mpsc::sync_channel::<(Vec<u8>, Vec<u8>)>(8);
    let (spare_y_tx, spare_y) = mpsc::channel::<Vec<u8>>();
    let watch = thread::scope(|s| {
        let shared = &tracking;
        let detecting = s.spawn(move || -> Result<(), String> {
            let (gw, gh) = (W / 4, H / 4);
            for (rgb, k) in batches {
                let maps = detector.run(&rgb)?;
                let _ = spare_tx.send(rgb);
                let mut t = shared.lock().map_err(|_| "the tracker failed")?;
                for i in 0..k {
                    let score = &maps.score[i * gw * gh..(i + 1) * gw * gh];
                    t.maps(score, &maps.reg[i * 4 * gw * gh..(i + 1) * 4 * gw * gh], gw, gh);
                }
                drop(t);
                let n = done.fetch_add(k, Ordering::Relaxed) + k;
                if n / PROGRESS_EVERY != (n - k) / PROGRESS_EVERY {
                    progress("tracking", n, total);
                }
            }
            Ok(())
        });
        let watching = s.spawn(move || {
            let mut watching = review.watching(run, keys);
            for (y, rows) in watch_frames {
                watching.frame(&y, &rows);
                let _ = spare_y_tx.send(y);
            }
            watching.part()
        });
        let decoded = (|| -> Result<(), String> {
            let stopped = || "the detector stopped".to_string();
            let mut convert = Converter::new(info.width, info.height, info.matrix, info.full);
            let mut yuv = vec![0u8; frame_bytes(info)];
            let mut rgb = vec![0u8; rgb_bytes];
            let mut waiting = vec![0u8; batch * rgb_bytes];
            let mut count = 0;
            while frames.next_into(&mut yuv)? {
                let next = shared.lock().map_err(|_| "the tracker failed")?.next_frame();
                if next == NextFrame::Stop {
                    break;
                }
                convert.rgb24(&yuv, &mut rgb);
                let mut y = spare_y.try_recv().unwrap_or_else(|_| vec![0u8; y_bytes]);
                y.copy_from_slice(&yuv[..y_bytes]);
                to_watch.send((y, rgb[rows.clone()].to_vec())).map_err(|_| "the camera watch stopped")?;
                if next == NextFrame::Watch {
                    continue;
                }
                shared.lock().map_err(|_| "the tracker failed")?.watch(&rgb);
                waiting[count * rgb_bytes..(count + 1) * rgb_bytes].copy_from_slice(&rgb);
                count += 1;
                if count == batch {
                    let next = spare.try_recv().unwrap_or_else(|_| vec![0u8; batch * rgb_bytes]);
                    to_detector.send((std::mem::replace(&mut waiting, next), count)).map_err(|_| stopped())?;
                    count = 0;
                }
            }
            if count > 0 {
                to_detector.send((waiting, count)).map_err(|_| stopped())?;
            }
            Ok(())
        })();
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
