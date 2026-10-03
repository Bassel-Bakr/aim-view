import { JobStage, Tracks } from '../../api';

/** Where the browser runs the detector: the GPU (WebGPU) or the CPU (WebAssembly). */
export type BrowserDevice = 'webgpu' | 'wasm';

/**
 * What the review worker is asked: a recording's file, where the core, the detector runtime and the model are, where
 * to run the detector and how many frames it takes at once, the scenario's target count (null: not known), and the
 * port to the camera worker.
 */
export interface ReviewRequest {
  file: Blob;
  coreUrl: string;
  ortPath: string;
  modelUrl: string;
  device: BrowserDevice;
  batch: number;
  cap: number | null;
  camera: MessagePort;
}

/** A recording's frames as the decoder gives them, as the core's converter takes them (src/wasm.rs: converter_new). */
export interface FrameFormat {
  width: number;
  height: number;
  matrix: number;
  full: number;
}

/** The camera worker's start: where the core is, the frames' format, and the fixed map (1280 x 720). */
export interface CameraStart extends FrameFormat {
  kind: 'start';
  coreUrl: string;
  fixed: Uint8Array;
}

/**
 * A frame, as much of it as the camera watch reads: the decoded Y plane (the recording's size), then the rows of its
 * 720p RGB the countdown test reads (core: camera_rgb_rows). Its buffer comes back once read.
 */
export interface CameraFrame {
  kind: 'frame';
  frame: ArrayBuffer;
}

/** No more frames: the tracks (tracker_finish's JSON), which the readings need. */
export interface CameraFinish {
  kind: 'finish';
  frames: string;
}

/** What the review worker tells the camera worker. */
export type CameraTask = CameraStart | CameraFrame | CameraFinish;

/** A frame's buffer, read and free again. */
export interface CameraFree {
  kind: 'free';
  frame: ArrayBuffer;
}

/** The readings, once every frame is read. */
export interface CameraDone {
  kind: 'readings';
  readings: VideoReadings;
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

/** The tracks, the video's readings, and the timings of the run (seconds). */
export interface ReviewTracked {
  kind: 'done';
  tracks: Tracks;
  readings: VideoReadings;
  seconds: number;
  keyFrames: number;
}

export interface ReviewFailed {
  kind: 'error';
  error: string;
}

/** What the review worker says back. */
export type ReviewMessage = ReviewProgress | ReviewTracked | ReviewFailed;

/** What the camera worker says back. */
export type CameraReply = CameraFree | CameraDone | ReviewFailed;
