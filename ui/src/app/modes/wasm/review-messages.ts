import { JobStage, Tracks } from '../../api';

/**
 * What the review worker is asked: a recording's file, where the core, the detector runtime and the model are, and
 * the scenario's target count (null: not known).
 */
export interface ReviewRequest {
  file: Blob;
  coreUrl: string;
  ortPath: string;
  modelUrl: string;
  cap: number | null;
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
