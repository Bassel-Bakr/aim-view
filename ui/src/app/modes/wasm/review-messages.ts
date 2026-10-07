/**
 * The messages between the page, the review workers and the camera workers, and the types they
 * carry. In: the page's review request (browser-review.ts). Out: the review workers' progress and
 * run parts, which core-module.ts joins; the camera link's tasks and the camera worker's replies.
 */

import { AreaBox, JobStage, Kind, TimeWindow } from '../../api';

/** Where the browser runs the detector: the GPU (WebGPU) or the CPU (WebAssembly). */
export type BrowserDevice = 'webgpu' | 'wasm';

/**
 * What the review worker is asked: a recording's file, which of its runs to review (`run` of
 * `runs`: src/session.rs; each run has a worker of its own) and the part of it to review (null:
 * all of it), where the core, the detector runtime and the model are, where to run the detector and
 * how many frames it takes at once, the scenario's target count (null: not known), the areas the
 * review ignores, the scenario's kind (null: not known; the model's at-crosshair rule may name the
 * kinds it is for), and the port to the camera worker.
 */
export interface ReviewRequest {
  /** The recording's video file. */
  file: Blob;
  /** Which run this worker reviews, from 0. */
  run: number;
  /** How many runs the review is split into, one worker each. */
  runs: number;
  /** The run window to review (with a margin the core adds); null for the whole recording. */
  window: TimeWindow | null;
  /** The address of the core's WebAssembly. */
  coreUrl: string;
  /** The folder onnxruntime-web loads its own WebAssembly files from. */
  ortPath: string;
  /** The address of the detector model's ONNX file (its _u8in export). */
  modelUrl: string;
  /** Where the detector runs. */
  device: BrowserDevice;
  /** How many frames the detector takes in one call. */
  batch: number;
  /** The scenario's target count; null when it is not known. */
  cap: number | null;
  /** The areas the review ignores, as shares of the frame. */
  areas: AreaBox[];
  /** The scenario's kind; null when it is not known. */
  kind: Kind | null;
  /** This worker's end of the port to its camera worker. */
  camera: MessagePort;
}

export type { FrameFormat } from '../../generated/frame-format';
export type { ReviewSetup } from '../../generated/review-setup';
export type { VideoRun } from '../../generated/video-run';

/**
 * The camera worker's opening, while the review worker reads the key frames: where the core is (it
 * loads meanwhile).
 */
export interface WatchOpen {
  /** Tells the task apart. */
  kind: 'open';
  /** The address of the core's WebAssembly. */
  coreUrl: string;
}

/**
 * The run's watches' start, after the key frames: the review's setup (src/session.rs: `Setup`, as
 * JSON), which of its runs this is, and what the key frames gave: the fixed map (1280 x 720) and
 * where the HUD's boxes are (`HudKeys`, as JSON).
 */
export interface WatchStart {
  /** Tells the task apart. */
  kind: 'start';
  /** The review's setup, as JSON. */
  setup: string;
  /** Which of the review's runs this is, from 0. */
  run: number;
  /** The fixed map, a byte a pixel at 1280 x 720 (1 fixed). */
  fixed: Uint8Array;
  /** Where the HUD's boxes are, as JSON (keys_finish's). */
  hud: string;
}

/**
 * A frame the run reads, as much of it as the watches read: the decoded Y plane (the recording's
 * size), then the rows of its 720p RGB the countdown test reads (core: camera_rgb_rows). Its
 * buffer comes back once read.
 */
export interface CameraFrame {
  /** Tells the task apart. */
  kind: 'frame';
  /** The frame's bytes, moved to the camera worker (from CameraLink.take). */
  frame: ArrayBuffer;
}

/** No more frames: the watches' part comes back. */
export interface CameraFinish {
  /** Tells the task apart. */
  kind: 'finish';
}

/** What the review worker tells the camera worker, in this order: open, start, frames, finish. */
export type CameraTask = WatchOpen | WatchStart | CameraFrame | CameraFinish;

/** A frame's buffer, read and free again. */
export interface CameraFree {
  /** Tells the reply apart. */
  kind: 'free';
  /** The frame's buffer, moved back for the next frame. */
  frame: ArrayBuffer;
}

/** The watches' part of the run (src/wasm.rs: watching_part's JSON), once every frame is read. */
export interface CameraDone {
  /** Tells the reply apart. */
  kind: 'part';
  /** The watches' part, as JSON. */
  part: string;
}

/** How far a review worker is: its stage and the frames done of total. */
export interface ReviewProgress {
  /** Tells the message apart. */
  kind: 'progress';
  /** What the worker is doing: 'looking' (the key frames), then 'tracking'. */
  stage: JobStage;
  /** The frames this worker has tracked so far. */
  done: number;
  /** The frames all the review's runs track, together. */
  total: number;
}

/** The room's move on screen since the frame before (degrees), and how many tiles agreed on it. */
export type CameraShift = [dx: number, dy: number, tiles: number];

/** The camera's turn in a frame, or null where it could not be read. */
export type CameraReading = CameraShift | null;

/**
 * What a tracking run reads from the video besides the tracks: per frame, the camera's reading and
 * whether KovaaK's countdown bar shows (src/camera.rs).
 */
export interface VideoReadings {
  /** Each frame's camera reading, in frame order. */
  camera: CameraReading[];
  /** Whether KovaaK's countdown bar shows in each frame. */
  countdown: boolean[];
}

export type { HudFinal } from '../../generated/hud-final';
export type { HudGame } from '../../generated/hud-game';
export type { HudReading } from '../../generated/hud-reading';

/**
 * A run's part of the review, for the page to join with the other runs' in order
 * (core-module.ts: joinReview): the review's setup (every run's is the same; src/session.rs:
 * `Setup`, as JSON), its tracking's and its watches' parts (src/wasm.rs: tracking_part's and
 * watching_part's JSON), the key frames' fixed map (1280 x 720) and where the detector ran.
 */
export interface RunPart {
  /** The review's setup, as JSON. */
  setup: string;
  /** The tracking's part, as JSON. */
  track: string;
  /** The watches' part, as JSON. */
  watch: string;
  /** The key frames' fixed map, a byte a pixel at 1280 x 720. */
  fixed: Uint8Array;
  /** Where the detector ran (WebGPU can fall back to the CPU). */
  device: BrowserDevice;
}

/** A run's part; null for a run the recording is too short to have. */
export interface ReviewPart {
  /** Tells the message apart. */
  kind: 'part';
  /** The run's part; null when the recording has no such run. */
  part: RunPart | null;
}

/** A worker failed: why, in words. */
export interface ReviewFailed {
  /** Tells the message apart. */
  kind: 'error';
  /** The error's message. */
  error: string;
}

/** What the review worker says back. */
export type ReviewMessage = ReviewProgress | ReviewPart | ReviewFailed;

/** What the camera worker says back. */
export type CameraReply = CameraFree | CameraDone | ReviewFailed;
