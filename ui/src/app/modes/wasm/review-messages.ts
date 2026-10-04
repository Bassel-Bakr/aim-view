import { AreaBox, JobStage, TimeWindow } from '../../api';

/** Where the browser runs the detector: the GPU (WebGPU) or the CPU (WebAssembly). */
export type BrowserDevice = 'webgpu' | 'wasm';

/**
 * What the review worker is asked: a recording's file, which of its runs to review (`run` of `runs`: split-runs.ts;
 * each run has a worker of its own) and the part of it to review (null: all of it), where the core, the detector
 * runtime and the model are, where to run the detector and how many frames it takes at once, the scenario's target
 * count (null: not known), the areas the review ignores, and the port to the camera worker.
 */
export interface ReviewRequest {
  file: Blob;
  run: number;
  runs: number;
  window: TimeWindow | null;
  coreUrl: string;
  ortPath: string;
  modelUrl: string;
  device: BrowserDevice;
  batch: number;
  cap: number | null;
  areas: AreaBox[];
  camera: MessagePort;
}

/** A recording's frames as the decoder gives them, as the core's converter takes them (src/wasm.rs: converter_new). */
export interface FrameFormat {
  width: number;
  height: number;
  matrix: number;
  full: number;
}

/**
 * The camera worker's opening, before the key frames: where the core is and the frames' format. It makes the HUD watch,
 * which reads every key frame before any frame (for where the HUD's boxes are).
 */
export interface WatchOpen extends FrameFormat {
  kind: 'open';
  coreUrl: string;
}

/** A key frame, in the fixed map's pass: its decoded Y plane (the recording's size). Its buffer comes back once read. */
export interface KeyFrame {
  kind: 'key';
  frame: ArrayBuffer;
}

/**
 * The camera watch's start, after the key frames: the fixed map (1280 x 720), the frames before the review's first,
 * which neither watch sees (a review from part way in; 0 but for the first run of such a review), and the recording's
 * excluded areas, which the camera's tiles keep clear of.
 */
export interface CameraStart {
  kind: 'start';
  fixed: Uint8Array;
  skip: number;
  areas: AreaBox[];
}

/**
 * A frame, as much of it as the watches read: the decoded Y plane (the recording's size; the HUD watch reads it as it
 * is), then the rows of its 720p RGB the countdown test reads (core: camera_rgb_rows). Its buffer comes back once read.
 */
export interface CameraFrame {
  kind: 'frame';
  frame: ArrayBuffer;
}

/** No more frames: the watches' parts come back. */
export interface CameraFinish {
  kind: 'finish';
}

/** What the review worker tells the camera worker, in this order: open, key frames, start, frames, finish. */
export type CameraTask = WatchOpen | KeyFrame | CameraStart | CameraFrame | CameraFinish;

/** A frame's (or key frame's) buffer, read and free again. */
export interface CameraFree {
  kind: 'free';
  frame: ArrayBuffer;
}

/** The camera and HUD watches' parts of the run (src/wasm.rs: camera_part's and hud_part's JSON). */
export interface WatchParts {
  camera: string;
  hud: string;
}

/** The watches' parts, once every frame is read. */
export interface CameraDone {
  kind: 'part';
  part: WatchParts;
}

export interface ReviewProgress {
  kind: 'progress';
  stage: JobStage;
  done: number;
  total: number;
}

/** The room's move on screen since the frame before (degrees), and how many tiles agreed on it. */
export type CameraShift = [dx: number, dy: number, tiles: number];

/** The camera's turn in a frame, or null where it could not be read. */
export type CameraReading = CameraShift | null;

/** What a tracking run reads from the video besides the tracks: per frame, the camera's reading and whether KovaaK's
 * countdown bar shows (src/camera.rs). */
export interface VideoReadings {
  camera: CameraReading[];
  countdown: boolean[];
}

export type { HudFinal } from '../../generated/hud-final';
export type { HudGame } from '../../generated/hud-game';
export type { HudReading } from '../../generated/hud-reading';

/**
 * A run's part of the review, for the page to join with the other runs' in order (core-module.ts: joinRuns): its
 * frames, its tracker's, camera watch's and HUD watch's parts (src/wasm.rs: tracker_part, camera_part and hud_part's
 * JSON), and what every run finds the same: the frame rate, the fixed map (1280 x 720), the frames' format, where the
 * detector ran and the key frames read.
 */
export interface RunPart {
  frames: number;
  track: string;
  camera: string;
  hud: string;
  fps: number;
  fixed: Uint8Array;
  format: FrameFormat;
  device: BrowserDevice;
  keyFrames: number;
}

/** A run's part; null for a run the recording is too short to have. */
export interface ReviewPart {
  kind: 'part';
  part: RunPart | null;
}

export interface ReviewFailed {
  kind: 'error';
  error: string;
}

/** What the review worker says back. */
export type ReviewMessage = ReviewProgress | ReviewPart | ReviewFailed;

/** What the camera worker says back. */
export type CameraReply = CameraFree | CameraDone | ReviewFailed;
