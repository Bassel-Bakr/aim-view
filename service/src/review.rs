//! A recording's review on this computer (the browser's review worker, natively): ffmpeg decodes the frames, the core
//! converts them to ffmpeg's 720p RGB and luma byte for byte, the detector runs on the GPU (detector.rs), the tracker
//! keeps and links the targets, the camera watch reads the camera's turn and the HUD watch the game's on-screen counts.
//! A recording is split into runs at key frames (`split_runs`, as ui/src/app/modes/wasm/split-runs.ts), reviewed at
//! once and joined: one ffmpeg decoder is the limit, as one browser decoder was.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, mpsc};
use std::thread;

use aimview::areas::{AreaFinder, Found, sample_frames};
use aimview::camera::{COUNTDOWN_ROWS, CameraPart, CameraWatch, VideoReadings, excluded};
use aimview::convert::{Converter, DST_H as H, DST_W as W};
use aimview::fixed::FixedMap;
use aimview::hud::{HudPart, HudReading, HudWatch};
use aimview::model::ModelSettings;
use aimview::track::{Mask, TrackFrame};
use aimview::tracker::{TrackPart, Tracker};
use serde::Serialize;

use crate::config::Device;
use crate::detector::{Detector, model_settings};
use crate::video::{Frames, VideoInfo, probe};

/// The fewest frames a run has (10 s at 60 frames a second): a shorter recording is one run.
pub const LEAST_RUN: usize = 600;
/// Frames between progress reports.
const PROGRESS_EVERY: usize = 60;

/// What to review: the video, the detector model (its _u8in export) and the device it runs on, the frames it takes at
/// once, the scenario's target count (0: not known), the runs to split the recording into, the part of the video to
/// track (the user's run window with a margin; None: all of it), and the areas it leaves out (the recording's,
/// areas.rs).
pub struct Request {
    pub video: PathBuf,
    pub model: PathBuf,
    pub device: Device,
    pub batch: usize,
    pub cap: usize,
    pub runs: usize,
    pub window: Option<TimeWindow>,
    pub areas: Vec<AreaBox>,
}

/// An area the review leaves out: [x0, y0, x1, y1] as shares of the frame, and its kind's id.
pub type AreaBox = (f64, f64, f64, f64, String);

/// A part of a video, in seconds.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, serde::Deserialize)]
pub struct TimeWindow {
    pub start: f64,
    pub end: f64,
}

/// The frames to review: from `first` up to `end` (not included), as indexes in the recording.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameRange {
    pub first: usize,
    pub end: usize,
}

/// The frames a time window holds: from the first at or after its start to the last at or before its end; all of them
/// for no window, or one with no frames.
pub fn window_frames(times: &[f64], window: Option<TimeWindow>) -> FrameRange {
    let all = FrameRange { first: 0, end: times.len() };
    let Some(w) = window else { return all };
    let Some(first) = times.iter().position(|&t| t >= w.start) else { return all };
    let end = times.iter().position(|&t| t > w.end).unwrap_or(times.len());
    if end <= first { all } else { FrameRange { first, end } }
}

/// The tracks as tracks.json keeps them: the frame rate, each frame's targets, the share of the frame the fixed map
/// covers, and the detector that found them.
#[derive(Serialize)]
pub struct Tracks {
    pub fps: f64,
    pub frames: Vec<TrackFrame>,
    pub fixed: f64,
    pub detector: String,
    /// The part of the video tracked, when only part of it was; the frames outside are empty.
    pub window: Option<TimeWindow>,
    /// The review's version (aimview::track::REVIEW_VERSION).
    pub version: u32,
    /// The areas it left out (a review is made again when the recording's change).
    pub areas: Vec<AreaBox>,
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

/// A run of the recording's frames: from a key frame's time (`from`) to the next run's (`to`, None for the last),
/// its first frame's index in the recording and how many frames it has.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub from: f64,
    pub to: Option<f64>,
    pub first: usize,
    pub frames: usize,
}

/// The recording's frames in `range` split into up to `parts` runs, each from a key frame: the first from the key frame
/// at or before the range's first frame (decoding starts at a key frame), each cut at the key frame nearest its share of
/// the frames, and none that would leave a run of fewer than `least` frames.
pub fn split_runs(times: &[f64], keys: &[f64], parts: usize, least: usize, range: FrameRange) -> Vec<Run> {
    let begin = keys
        .iter()
        .filter_map(|&k| times.iter().position(|&t| t == k))
        .filter(|&i| i <= range.first)
        .max()
        .unwrap_or(0);
    let n = range.end - begin;
    let mut starts = vec![begin];
    for i in 1..parts {
        let want = times[begin + n * i / parts];
        let best = keys
            .iter()
            .copied()
            .filter(|&k| k > times[begin])
            .min_by(|a, b| (a - want).abs().total_cmp(&(b - want).abs()));
        let Some(at) = best.and_then(|k| times.iter().position(|&t| t == k)) else { continue };
        if at >= starts[starts.len() - 1] + least && range.end >= at + least {
            starts.push(at);
        }
    }
    (0..starts.len())
        .map(|i| {
            let next = starts.get(i + 1).copied();
            Run {
                from: if starts[i] > 0 { times[starts[i]] } else { 0.0 },
                to: next.map(|j| times[j]),
                first: starts[i],
                frames: next.unwrap_or(range.end) - starts[i],
            }
        })
        .collect()
}

impl Request {
    /// The areas as the tracker takes them.
    fn rects(&self) -> Vec<[f64; 4]> {
        self.areas.iter().map(|a| [a.0, a.1, a.2, a.3]).collect()
    }

    /// The tracker for the areas and the model's settings: the challenge's end screen among the areas is left out only
    /// while it shows.
    fn tracker(&self, model: &ModelSettings) -> Tracker {
        let ends: Vec<bool> = self.areas.iter().map(|a| a.4 == aimview::popup::END_SCREEN).collect();
        let mut tracker = Tracker::new(self.rects(), self.cap).end_screens(&ends);
        tracker.set_model(model.clone());
        tracker
    }

    /// The camera watch, its tiles kept clear of the areas (KovOBS's layout when there are none, as python/review.py
    /// does) and of the fixed map.
    fn camera_watch(&self, fixed: &[u8]) -> CameraWatch {
        let rects = if self.areas.is_empty() { aimview::geometry::overlay_shares() } else { self.rects() };
        CameraWatch::new(&excluded(Mask::without(&rects).kept(), fixed))
    }
}

/// The size of one frame as ffmpeg gives it (YUV 4:2:0 at the video's size).
pub(crate) fn frame_bytes(info: &VideoInfo) -> usize {
    info.width * info.height + 2 * info.width.div_ceil(2) * info.height.div_ceil(2)
}

/// Reviews a recording: its tracks and readings. `on_device` is told where each run's detector runs.
pub fn review(req: &Request, progress: Progress, on_device: DeviceNote) -> Result<Reviewed, String> {
    let model = model_settings(&req.model)?;
    let info = probe(&req.video)?;
    if info.times.is_empty() {
        return Err("the video has no frames".into());
    }
    let range = window_frames(&info.times, req.window);
    let runs = split_runs(&info.times, &info.keys, req.runs.max(1), LEAST_RUN, range);
    let total = runs.iter().map(|r| r.frames).sum();
    progress("looking", 0, total);
    // each run's HUD watch reads every key frame before its frames
    let mut huds: Vec<HudWatch> = runs.iter().map(|_| HudWatch::new(info.width, info.height, info.full)).collect();
    // the area finder reads the same key frames, when there are enough of them (python/areas.py: sample)
    let mut finder = sample_frames(info.keys.len(), &info.times, info.duration).is_none().then(AreaFinder::new);
    let fixed = fixed_map(&req.video, &info, |small, y| {
        huds.iter_mut().for_each(|h| h.add_key(y));
        if let Some(f) = finder.as_mut() {
            f.add(small);
        }
    })?;
    let session = huds.first_mut().and_then(HudWatch::session_box);
    let finding = thread::spawn(move || finder.map(|f| f.finish(session)));
    let done = AtomicUsize::new(0);
    let parts: Vec<Result<RunPart, String>> = thread::scope(|s| {
        let running: Vec<_> = runs
            .iter()
            .zip(huds)
            .enumerate()
            // a review from part way in: the first run's camera and HUD watches have nothing before its first frame
            .map(|(i, (run, hud))| {
                let skip = if i == 0 { run.first } else { 0 };
                let (model, info, fixed, done) = (&model, &info, &fixed, &done);
                s.spawn(move || review_run(req, model, info, fixed, run, hud, skip, done, total, progress, on_device))
            })
            .collect();
        running.into_iter().map(|r| r.join().unwrap_or_else(|_| Err("a run of the review failed".into()))).collect()
    });
    progress("linking", total, total);
    let mut tracker = req.tracker(&model);
    let mut camera = req.camera_watch(&fixed);
    let mut hud = HudWatch::new(info.width, info.height, info.full);
    let mut devices = String::new();
    for (run, part) in runs.iter().zip(parts) {
        let part = part?;
        if tracker.add_part(part.track) != run.frames {
            return Err("the review's runs do not join up".into());
        }
        camera.join(part.camera);
        hud.join(part.hud);
        add_device(&mut devices, part.device);
    }
    let frames = tracker.finish();
    let readings = camera.finish(&frames);
    if readings.countdown.len() != frames.len() {
        return Err("the camera watch's runs do not join up".into());
    }
    if hud.frames() != frames.len() {
        return Err("the HUD watch's runs do not join up".into());
    }
    let share = fixed.iter().map(|&v| v as f64).sum::<f64>() / fixed.len() as f64;
    let detector = format!("onnxruntime ({devices})");
    let (window, version, areas) = (req.window, aimview::track::REVIEW_VERSION, req.areas.clone());
    let tracks = Tracks { fps: info.fps, frames, fixed: share, detector, window, version, areas };
    let found = finding.join().map_err(|_| "the area finder failed")?;
    Ok(Reviewed { tracks, readings, hud: hud.finish(), found })
}

/// A key frame: at 720p for the fixed map, and as decoded (the HUD watches read its Y plane).
type KeyFrame = (Vec<u8>, Vec<u8>);

/// The fixed map, from the key frames (as python/review.py builds it), and each key frame handed to `key`, in order: at
/// 1280 x 720 (YUV 4:2:0), and its Y plane at the video's size. Each key frame is decoded on its own, from its exact time (ffmpeg's libaom ignores
/// `-skip_frame nokey` and gives every frame), a few at once: decoder i takes every AT_ONCE-th key frame from the i-th
/// and waits while its next one is not wanted yet, so only a few whole frames are held at a time.
pub(crate) fn fixed_map(video: &Path, info: &VideoInfo, mut key: impl FnMut(&[u8], &[u8])) -> Result<Vec<u8>, String> {
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
            let mut map = FixedMap::default();
            for i in 0..info.keys.len() {
                let (small, yuv) = decoded[i % AT_ONCE].recv().map_err(|_| "a key frame failed".to_string())??;
                map.add(&small);
                key(&small, &yuv[..info.width * info.height]);
            }
            Ok(map.map())
        })();
        // the decoders stop once nothing takes their frames
        drop(decoded);
        for d in decoders {
            let _ = d.join();
        }
        taken
    })
}

/// A run's part of the review: its tracker's, camera watch's and HUD watch's parts, and where its detector ran.
struct RunPart {
    track: TrackPart,
    camera: CameraPart,
    hud: HudPart,
    device: &'static str,
}

/// One run: this thread decodes and converts each frame and watches its areas; a detector thread takes the frames a
/// batch at a time and hands their maps to the tracker in order; a camera thread reads the camera's turn and the HUD
/// from each frame's Y plane. A run but the last also reads the next run's first frame, for the camera's turn into it.
#[allow(clippy::too_many_arguments)]
fn review_run(
    req: &Request,
    model: &ModelSettings,
    info: &VideoInfo,
    fixed: &[u8],
    run: &Run,
    mut hud: HudWatch,
    skip: usize,
    done: &AtomicUsize,
    total: usize,
    progress: Progress,
    on_device: DeviceNote,
) -> Result<RunPart, String> {
    let rgb_bytes = W * H * 3;
    let batch = req.batch.max(1);
    let extra = usize::from(run.to.is_some());
    let mut frames = Frames::open(&req.video, (run.first > 0).then_some(run.from), Some(run.frames + extra))?;
    let mut detector = Detector::new(&req.model, batch, fixed, req.device)?;
    let device = detector.device;
    on_device(device);
    let mut started = req.tracker(model);
    started.start_at(run.first);
    let tracker = Mutex::new(started);
    let (rows_from, rows_to) = (COUNTDOWN_ROWS.0 * W * 3, COUNTDOWN_ROWS.1 * W * 3);
    // a batch's frames, and how many of them are the run's (the rest of the last batch is left over)
    let (to_detector, batches) = mpsc::sync_channel::<(Vec<u8>, usize)>(2);
    let (spare_tx, spare) = mpsc::channel::<Vec<u8>>();
    // a frame's Y plane (at the video's size) and its countdown rows; the Y planes come back to be filled again
    let y_bytes = info.width * info.height;
    let (to_camera, camera_frames) = mpsc::sync_channel::<(Vec<u8>, Vec<u8>)>(8);
    let (spare_y_tx, spare_y) = mpsc::channel::<Vec<u8>>();
    let (camera, hud) = thread::scope(|s| {
        let shared = &tracker;
        let detecting = s.spawn(move || -> Result<(), String> {
            let (gw, gh) = (W / 4, H / 4);
            for (rgb, k) in batches {
                let maps = detector.run(&rgb)?;
                let _ = spare_tx.send(rgb);
                let mut t = shared.lock().map_err(|_| "the tracker failed")?;
                for i in 0..k {
                    let score = &maps.score[i * gw * gh..(i + 1) * gw * gh];
                    t.push_maps(score, &maps.reg[i * 4 * gw * gh..(i + 1) * 4 * gw * gh], gw, gh);
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
            let mut camera = req.camera_watch(fixed);
            camera.skip(skip);
            hud.skip(skip);
            let mut convert = Converter::new(info.width, info.height, info.matrix, info.full);
            let mut luma = vec![0u8; W * H];
            let mut rgb = vec![0u8; rgb_bytes];
            for (y, rows) in camera_frames {
                convert.luma(&y, &mut luma);
                rgb[rows_from..rows_to].copy_from_slice(&rows);
                camera.add(&luma, &rgb);
                hud.add(&y);
                let _ = spare_y_tx.send(y);
            }
            (camera.part(), hud.part())
        });
        let decoded = (|| -> Result<usize, String> {
            let stopped = || "the detector stopped".to_string();
            let mut convert = Converter::new(info.width, info.height, info.matrix, info.full);
            let mut yuv = vec![0u8; frame_bytes(info)];
            let mut rgb = vec![0u8; rgb_bytes];
            let mut waiting = vec![0u8; batch * rgb_bytes];
            let (mut count, mut seen) = (0, 0);
            while frames.next_into(&mut yuv)? {
                convert.rgb24(&yuv, &mut rgb);
                let mut y = spare_y.try_recv().unwrap_or_else(|_| vec![0u8; y_bytes]);
                y.copy_from_slice(&yuv[..y_bytes]);
                to_camera.send((y, rgb[rows_from..rows_to].to_vec())).map_err(|_| "the camera watch stopped")?;
                seen += 1;
                if seen > run.frames {
                    break;
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
            Ok(seen)
        })();
        drop(to_detector);
        drop(to_camera);
        let detected = detecting.join().unwrap_or_else(|_| Err("the detector failed".into()));
        let watched = watching.join().map_err(|_| "the camera watch failed")?;
        detected?;
        let seen = decoded?;
        if seen != run.frames + extra {
            return Err(format!("the run from {:.3} s gave {seen} frames where it has {}", run.from, run.frames + extra));
        }
        Ok(watched)
    })?;
    let track = tracker.into_inner().map_err(|_| "the tracker failed")?.part();
    Ok(RunPart { track, camera, hud, device })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// n frames at 60 a second from 0, a key frame every `every` frames.
    fn video(n: usize, every: usize) -> (Vec<f64>, Vec<f64>) {
        let times: Vec<f64> = (0..n).map(|i| i as f64 / 60.0).collect();
        let keys = times.iter().copied().step_by(every).collect();
        (times, keys)
    }

    /// The same runs as the browser's split (ui/src/app/modes/wasm/split-runs.spec.ts).
    #[test]
    fn runs_as_the_browser_splits_them() {
        let (times, keys) = video(6038, 240);
        let all = |n| FrameRange { first: 0, end: n };
        let runs = split_runs(&times, &keys, 2, 600, all(6038));
        assert_eq!(
            runs,
            vec![
                Run { from: 0.0, to: Some(times[3120]), first: 0, frames: 3120 },
                Run { from: times[3120], to: None, first: 3120, frames: 2918 },
            ]
        );
        let (times, keys) = video(900, 240);
        assert_eq!(split_runs(&times, &keys, 2, 600, all(900)).len(), 1);
        let (times, keys) = video(9000, 300);
        let firsts: Vec<usize> = split_runs(&times, &keys, 3, 600, all(9000)).iter().map(|r| r.first).collect();
        assert_eq!(firsts, vec![0, 3000, 6000]);
    }

    /// A window: from the key frame before it to its end, as the browser splits it (split-runs.spec.ts).
    #[test]
    fn runs_in_a_window_as_the_browser_splits_them() {
        let (times, keys) = video(6000, 240);
        let range = window_frames(&times, Some(TimeWindow { start: 30.0, end: 80.0 }));
        assert_eq!(range, FrameRange { first: 1800, end: 4801 });
        assert_eq!(
            split_runs(&times, &keys, 2, 600, range),
            vec![
                Run { from: times[1680], to: Some(times[3120]), first: 1680, frames: 1440 },
                Run { from: times[3120], to: None, first: 3120, frames: 1681 },
            ]
        );
        let all = FrameRange { first: 0, end: 600 };
        assert_eq!(window_frames(&times[..600], Some(TimeWindow { start: 50.0, end: 60.0 })), all);
    }
}
