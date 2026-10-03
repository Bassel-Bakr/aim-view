//! A recording's review on this computer (the browser's review worker, natively): ffmpeg decodes the frames, the core
//! converts them to ffmpeg's 720p RGB and luma byte for byte, the detector runs on the GPU (DirectML), the tracker
//! keeps and links the targets, and the camera watch reads the camera's turn. A recording is split into runs at key
//! frames (`split_runs`, as ui/src/app/modes/wasm/split-runs.ts), reviewed at once and joined: one ffmpeg decoder is
//! the limit, as one browser decoder was.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, mpsc};
use std::thread;

use aimview::camera::{COUNTDOWN_ROWS, CameraPart, CameraWatch, VideoReadings};
use aimview::convert::{Converter, DST_H as H, DST_W as W};
use aimview::fixed::FixedMap;
use aimview::track::TrackFrame;
use aimview::tracker::{TrackPart, Tracker};
use serde::Serialize;

use crate::detector::Detector;
use crate::video::{Frames, VideoInfo, probe};

/// The fewest frames a run has (10 s at 60 frames a second): a shorter recording is one run.
pub const LEAST_RUN: usize = 600;
/// Frames between progress reports.
const PROGRESS_EVERY: usize = 60;

/// What to review: the video, the detector model (its _u8in export), the frames it takes at once, the scenario's
/// target count (0: not known) and the runs to split the recording into.
pub struct Request {
    pub video: PathBuf,
    pub model: PathBuf,
    pub batch: usize,
    pub cap: usize,
    pub runs: usize,
}

/// The tracks as tracks.json keeps them: the frame rate, each frame's targets, the share of the frame the fixed map
/// covers, and the detector that found them.
#[derive(Serialize)]
pub struct Tracks {
    pub fps: f64,
    pub frames: Vec<TrackFrame>,
    pub fixed: f64,
    pub detector: String,
}

/// A review's tracks and the video's readings.
pub struct Reviewed {
    pub tracks: Tracks,
    pub readings: VideoReadings,
}

/// Where a review stands: its stage ("looking" at the key frames, "tracking", "linking"), frames done, of how many.
pub type Progress<'a> = &'a (dyn Fn(&str, usize, usize) + Sync);

/// A run of the recording's frames: from a key frame's time (`from`) to the next run's (`to`, None for the last),
/// its first frame's index in the recording and how many frames it has.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub from: f64,
    pub to: Option<f64>,
    pub first: usize,
    pub frames: usize,
}

/// The recording's frames split into up to `parts` runs, each from a key frame: the cut at the key frame nearest its
/// share of the frames, and none that would leave a run of fewer than `least` frames.
pub fn split_runs(times: &[f64], keys: &[f64], parts: usize, least: usize) -> Vec<Run> {
    let n = times.len();
    let mut starts = vec![0];
    for i in 1..parts {
        let want = times[n * i / parts];
        let best = keys
            .iter()
            .copied()
            .filter(|&k| k > times[0])
            .min_by(|a, b| (a - want).abs().total_cmp(&(b - want).abs()));
        let Some(at) = best.and_then(|k| times.iter().position(|&t| t == k)) else { continue };
        if at - starts[starts.len() - 1] >= least && n - at >= least {
            starts.push(at);
        }
    }
    (0..starts.len())
        .map(|i| {
            let next = starts.get(i + 1).copied();
            Run {
                from: if i > 0 { times[starts[i]] } else { 0.0 },
                to: next.map(|j| times[j]),
                first: starts[i],
                frames: next.unwrap_or(n) - starts[i],
            }
        })
        .collect()
}

/// The size of one frame as ffmpeg gives it (YUV 4:2:0 at the video's size).
fn frame_bytes(info: &VideoInfo) -> usize {
    info.width * info.height + 2 * info.width.div_ceil(2) * info.height.div_ceil(2)
}

/// Reviews a recording: its tracks and readings.
pub fn review(req: &Request, progress: Progress) -> Result<Reviewed, String> {
    let info = probe(&req.video)?;
    let total = info.times.len();
    if total == 0 {
        return Err("the video has no frames".into());
    }
    progress("looking", 0, total);
    let fixed = fixed_map(&req.video, &info)?;
    let runs = split_runs(&info.times, &info.keys, req.runs.max(1), LEAST_RUN);
    let done = AtomicUsize::new(0);
    let parts: Vec<Result<RunPart, String>> = thread::scope(|s| {
        let running: Vec<_> = runs
            .iter()
            .map(|run| s.spawn(|| review_run(req, &info, &fixed, run, &done, total, progress)))
            .collect();
        running.into_iter().map(|r| r.join().unwrap_or_else(|_| Err("a run of the review failed".into()))).collect()
    });
    progress("linking", total, total);
    let mut tracker = Tracker::kovobs(req.cap);
    let mut camera = CameraWatch::for_recording(&fixed);
    let mut device = "";
    for (run, part) in runs.iter().zip(parts) {
        let part = part?;
        if tracker.add_part(part.track) != run.frames {
            return Err("the review's runs do not join up".into());
        }
        camera.join(part.camera);
        device = part.device;
    }
    let frames = tracker.finish();
    let readings = camera.finish(&frames);
    if readings.countdown.len() != total {
        return Err("the camera watch's runs do not join up".into());
    }
    let share = fixed.iter().map(|&v| v as f64).sum::<f64>() / fixed.len() as f64;
    let tracks = Tracks { fps: info.fps, frames, fixed: share, detector: format!("onnxruntime ({device})") };
    Ok(Reviewed { tracks, readings })
}

/// The fixed map, from the key frames (as python/review.py builds it).
fn fixed_map(video: &Path, info: &VideoInfo) -> Result<Vec<u8>, String> {
    let mut convert = Converter::new(info.width, info.height, info.matrix, info.full);
    let mut frames = Frames::key_frames(video)?;
    let mut yuv = vec![0u8; frame_bytes(info)];
    let mut small = vec![0u8; W * H * 3 / 2];
    let mut map = FixedMap::default();
    while frames.next_into(&mut yuv)? {
        convert.yuv420p(&yuv, &mut small);
        map.add(&small);
    }
    Ok(map.map())
}

/// A run's part of the review: its tracker's and camera watch's parts, and where its detector ran.
struct RunPart {
    track: TrackPart,
    camera: CameraPart,
    device: &'static str,
}

/// One run: this thread decodes and converts each frame and watches its areas; a detector thread takes the frames a
/// batch at a time and hands their maps to the tracker in order; a camera thread reads the camera's turn. A run but
/// the last also reads the next run's first frame, for the camera's turn into it.
fn review_run(
    req: &Request,
    info: &VideoInfo,
    fixed: &[u8],
    run: &Run,
    done: &AtomicUsize,
    total: usize,
    progress: Progress,
) -> Result<RunPart, String> {
    let rgb_bytes = W * H * 3;
    let batch = req.batch.max(1);
    let extra = usize::from(run.to.is_some());
    let mut frames = Frames::open(&req.video, (run.first > 0).then_some(run.from), Some(run.frames + extra))?;
    let mut detector = Detector::new(&req.model, batch, fixed)?;
    let device = detector.device;
    let mut started = Tracker::kovobs(req.cap);
    started.start_at(run.first);
    let tracker = Mutex::new(started);
    let (rows_from, rows_to) = (COUNTDOWN_ROWS.0 * W * 3, COUNTDOWN_ROWS.1 * W * 3);
    // a batch's frames, and how many of them are the run's (the rest of the last batch is left over)
    let (to_detector, batches) = mpsc::sync_channel::<(Vec<u8>, usize)>(2);
    let (spare_tx, spare) = mpsc::channel::<Vec<u8>>();
    // a frame's luma and its countdown rows
    let (to_camera, camera_frames) = mpsc::sync_channel::<(Vec<u8>, Vec<u8>)>(8);
    let camera = thread::scope(|s| {
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
            let mut camera = CameraWatch::for_recording(fixed);
            let mut rgb = vec![0u8; rgb_bytes];
            for (luma, rows) in camera_frames {
                rgb[rows_from..rows_to].copy_from_slice(&rows);
                camera.add(&luma, &rgb);
            }
            camera.part()
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
                let mut luma = vec![0u8; W * H];
                convert.luma(&yuv[..info.width * info.height], &mut luma);
                to_camera.send((luma, rgb[rows_from..rows_to].to_vec())).map_err(|_| "the camera watch stopped")?;
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
        let camera = watching.join().map_err(|_| "the camera watch failed")?;
        detected?;
        let seen = decoded?;
        if seen != run.frames + extra {
            return Err(format!("the run from {:.3} s gave {seen} frames where it has {}", run.from, run.frames + extra));
        }
        Ok(camera)
    })?;
    let track = tracker.into_inner().map_err(|_| "the tracker failed")?.part();
    Ok(RunPart { track, camera, device })
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
        let runs = split_runs(&times, &keys, 2, 600);
        assert_eq!(
            runs,
            vec![
                Run { from: 0.0, to: Some(times[3120]), first: 0, frames: 3120 },
                Run { from: times[3120], to: None, first: 3120, frames: 2918 },
            ]
        );
        let (times, keys) = video(900, 240);
        assert_eq!(split_runs(&times, &keys, 2, 600).len(), 1);
        let (times, keys) = video(9000, 300);
        let firsts: Vec<usize> = split_runs(&times, &keys, 3, 600).iter().map(|r| r.first).collect();
        assert_eq!(firsts, vec![0, 3000, 6000]);
    }
}
