import { AreaBox, JobStage, TimeWindow } from '../../api';

/** Where the browser runs the detector: the GPU (WebGPU) or the CPU (WebAssembly). */
export type BrowserDevice = 'webgpu' | 'wasm';

/**
 * What the review worker is asked: a recording's file, which of its runs to review (`run` of `runs`: src/session.rs;
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

export type { FrameFormat } from '../../generated/frame-format';
export type { ReviewSetup } from '../../generated/review-setup';
export type { VideoRun } from '../../generated/video-run';

/**
 * The camera worker's opening, while the review worker reads the key frames: where the core is (it loads meanwhile).
 */
export interface WatchOpen {
  kind: 'open';
  coreUrl: string;
}

/**
 * The run's watches' start, after the key frames: the review's setup (src/session.rs: `Setup`, as JSON), which of its
 * runs this is, and what the key frames gave: the fixed map (1280 x 720) and where the HUD's boxes are (`HudKeys`, as
 * JSON).
 */
export interface WatchStart {
  kind: 'start';
  setup: string;
  run: number;
  fixed: Uint8Array;
  hud: string;
}

/**
 * A frame the run reads, as much of it as the watches read: the decoded Y plane (the recording's size), then the rows
 * of its 720p RGB the countdown test reads (core: camera_rgb_rows). Its buffer comes back once read.
 */
export interface CameraFrame {
  kind: 'frame';
  frame: ArrayBuffer;
}

/** No more frames: the watches' part comes back. */
export interface CameraFinish {
  kind: 'finish';
}

/** What the review worker tells the camera worker, in this order: open, start, frames, finish. */
export type CameraTask = WatchOpen | WatchStart | CameraFrame | CameraFinish;

/** A frame's buffer, read and free again. */
export interface CameraFree {
  kind: 'free';
  frame: ArrayBuffer;
}

/** The watches' part of the run (src/wasm.rs: watching_part's JSON), once every frame is read. */
export interface CameraDone {
  kind: 'part';
  part: string;
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
 * A run's part of the review, for the page to join with the other runs' in order (core-module.ts: joinReview): the
 * review's setup (every run's is the same; src/session.rs: `Setup`, as JSON), its tracking's and its watches' parts
 * (src/wasm.rs: tracking_part's and watching_part's JSON), the key frames' fixed map (1280 x 720) and where the detector
 * ran.
 */
export interface RunPart {
  setup: string;
  track: string;
  watch: string;
  fixed: Uint8Array;
  device: BrowserDevice;
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
