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

/** The tracks, and the timings of the run (seconds). */
export interface ReviewTracked {
  kind: 'done';
  tracks: Tracks;
  seconds: number;
  keyFrames: number;
}

export interface ReviewFailed {
  kind: 'error';
  error: string;
}

/** What the review worker says back. */
export type ReviewMessage = ReviewProgress | ReviewTracked | ReviewFailed;
