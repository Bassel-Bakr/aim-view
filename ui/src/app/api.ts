/** The review server's JSON API (python/server.py), typed. */

import { HttpErrorResponse } from '@angular/common/http';

/** Milliseconds per 1280 x 720 frame on each runtime. */
export interface ModelSpeed {
  gpu: number;
  cpu: number;
  browser: number;
}

/** A model's result on one check: [kills matched, flicks measured], or for tracking [mean, mean size]. */
export type CheckResult = [first: number, second: number];

export type CheckKey = 'static' | 'moving' | 'uploads' | 'tracking';

export type ModelChecks = Partial<Record<CheckKey, CheckResult>>;

export interface Model {
  name: string;
  label: string;
  available: boolean;
  default?: boolean;
  older?: boolean;
  params?: number;
  kb?: number | null;
  trained?: string;
  best?: string;
  weak?: string;
  speed_ms?: ModelSpeed;
  checks?: ModelChecks;
}

/** One of the checks every model was measured on. */
export interface Check {
  key: CheckKey;
  name: string;
  what: string;
  of?: number;
}

export type Device = 'cuda' | 'cpu';

export interface ModelList {
  chosen: string;
  device: Device;
  speed: string;
  checked_on: string;
  checks: Check[];
  models: Model[];
}

/** A scenario's kind, from the game's tags (review.scenario_kinds). */
export type Kind = 'static' | 'dynamic' | 'tracking' | 'switching';

/** A recording in the list (/api/vods). kind is null for a scenario the game no longer has. */
export interface Recording {
  id: string;
  scenario: string;
  kind: Kind | null;
  score: number | null;
  stamp: string;
  mtime: number;
  size: number;
  stats: boolean;
  analysed: boolean;
  not_aim: boolean;
  uploaded?: boolean;
}

/** How the video maps to angles: frame size, the crosshair's pixel, and the focal length in pixels. */
export interface Geometry {
  W: number;
  H: number;
  CX: number;
  CY: number;
  K: number;
}

/** A target's place in one frame: [frame, x, y], in degrees from the crosshair. */
export type PathPoint = [frame: number, x: number, y: number];

/** The frames from a bot's death until the crosshair is on a bot again. */
export type Switch = [start: number, end: number];

/** Where a review's kills and shots came from. */
export type Source = 'stats' | 'hud' | 'aimlab' | 'video';

export interface ReportInfo {
  source?: Source;
  matched?: number;
}

/** One kill and the flick to it (review.measure). Times in seconds, angles in degrees. */
export interface Flick {
  n: number;
  shots: number;
  D0: number;
  dir: number;
  total: number;
  react: number;
  flick: number;
  peak: number;
  end_left: number;
  end_off: number;
  arrive: number;
  dwell: number;
  past: number;
  corr: number;
  click_speed: number;
  click_off: number;
  settle: number;
  still: number;
  start_frame: number;
  kill_frame: number;
  spawned: boolean;
  hold: number;
  breaks: number;
  off: number;
}

export interface ClickSummary {
  scenario: string;
  score: number;
  kills: number;
  misses: number;
  accuracy: number;
  radius: number;
  measured: number;
  info: ReportInfo;
}

export interface TrackSummary {
  scenario: string;
  score: number;
  accuracy: number;
  on_target: number;
  start: number | null;
  end: number | null;
  switches: Switch[];
  info: ReportInfo;
}

/** What every report has. review_model: the model that made it (null: not recorded). */
export interface ReportBase {
  fps: number;
  geometry: Geometry;
  review_model: string | null;
}

/** A clicking run: one flick per kill. */
export interface ClickReport extends ReportBase {
  mode: 'click';
  summary: ClickSummary;
  flicks: Flick[];
  paths: Record<string, PathPoint[]>;
}

/** A tracking run: time on the bot, measured frame by frame. */
export interface TrackReport extends ReportBase {
  mode: 'track';
  summary: TrackSummary;
}

export type Report = ClickReport | TrackReport;

/** A target in a frame: [track id, x, y], in degrees from the crosshair. */
export type TrackPoint = [id: number, x: number, y: number];

/** A target's size in degrees: [width, height]. */
export type TargetSize = [width: number, height: number];

/** One frame's targets. wh: their sizes, s: the model's scores (when the model made them). */
export interface TrackFrame {
  i: number;
  t: TrackPoint[];
  wh?: TargetSize[];
  s?: number[];
}

/** Every target in every frame (/api/tracks). */
export interface Tracks {
  fps: number;
  frames: TrackFrame[];
}

export type JobStage =
  | 'none'
  | 'starting'
  | 'looking'
  | 'tracking'
  | 'linking'
  | 'reading the HUD'
  | 'camera'
  | 'measuring'
  | 'done'
  | 'error';

/** A review in progress, or its end (/api/job). */
export interface Job {
  stage: JobStage;
  done?: number;
  total?: number;
  seconds?: number;
  error?: string;
}

/** The body of an error response. */
export interface ApiError {
  error?: string;
}

/** What went wrong with a request: the server's own message when it sent one ({error}), else the request's. */
export function errorMessage(e: unknown): string {
  if (e instanceof HttpErrorResponse) return (e.error as ApiError | null)?.error ?? e.message;
  return e instanceof Error ? e.message : String(e);
}
