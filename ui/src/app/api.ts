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

/** Where the detector runs: the GPU, the CPU, or the browser (WebAssembly). */
/**
 * Where the detector runs: the server's GPU (cuda) or CPU, in the browser on the CPU (wasm) or the GPU (webgpu), or
 * in the desktop app on the GPU (directml).
 */
export type Device = 'cuda' | 'cpu' | 'wasm' | 'webgpu' | 'directml';

export interface ModelList {
  chosen: string;
  device: Device;
  /** The devices the user can choose between, where the mode lets them choose (the browser). */
  devices?: Device[];
  /** How many frames the detector takes in one go on this device, and the choices (the browser). */
  batch?: number;
  batches?: number[];
  speed: string;
  checked_on: string;
  checks: Check[];
  models: Model[];
  /** Why a model that is not available cannot run here; without it, it needs the GPU. */
  unavailable?: string;
}

/** A scenario's kind, from the game's tags (review.scenario_kinds). */
export type Kind = 'static' | 'dynamic' | 'tracking' | 'switching';

/** What a scenario's file says about its runs: its kind, time limit (seconds) and targets alive at once. */
export interface ScenarioInfo {
  kind: Kind;
  limit: number | null;
  targets: number | null;
}

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
  /** Opened from this computer: it stays in this browser and is gone when the page closes. */
  local?: boolean;
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
  /** Kills matched to a target in the video, and kills the source counted (null: none counted, the video alone). */
  matched?: number;
  kills_stats?: number | null;
}

/** A target's place from the crosshair, in degrees: [x, y]. */
export type TargetOffset = [x: number, y: number];

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
  /** Separate bursts of movement after the main flick. */
  corr: number;
  click_speed: number;
  /** How far the crosshair was from the target's center at the click, and where: the target's place from the
   * crosshair, in degrees. */
  click_off: number;
  click_off_xy: TargetOffset;
  settle: number;
  still: number;
  start_frame: number;
  kill_frame: number;
  spawned: boolean;
  hold: number;
  breaks: number;
  off: number;
  /** Where the kill's time went, as KillParts; null when one of its steps was not found. */
  parts: KillParts | null;
}

/** A kill's time in its five steps, in seconds: react, main flick, onto the target, settle, still on the target. */
export type KillParts = [react: number, flick: number, onto: number, settle: number, still: number];

/** The kills in one band of distance. short and past are shares; times in seconds. */
export interface DistanceBand {
  lo: number;
  hi: number;
  n: number;
  interval: number;
  react: number;
  short: number;
  past: number;
  still: number;
}

/** Compass direction of a flick, or of a target's motion. */
export type Direction =
  'right' | 'up-right' | 'up' | 'up-left' | 'left' | 'down-left' | 'down' | 'down-right';

/** The kills toward one direction. beyond: the median time over what the distance predicts (Fitts' law). */
export interface DirectionBand {
  name: Direction;
  n: number;
  interval: number;
  distance: number;
  short: number;
  past: number;
  beyond: number | null;
}

/** One of the review's checks: "attention" when the run should look at it. */
export interface Issue {
  issue?: number;
  title: string;
  value: string;
  flag: 'attention' | 'fine';
  why: string;
}

export interface ClickSummary {
  scenario: string;
  score: number | null;
  kills: number;
  misses: number | null;
  shots: number | null;
  accuracy: number | null;
  radius: number;
  measured: number;
  sens: string | null;
  fps_avg: number | null;
  median_interval: number | null;
  react: number | null;
  flick: number | null;
  peak: number | null;
  still: number | null;
  click_speed: number | null;
  click_off: number | null;
  /** The average kill's time in its five steps. */
  budget: KillParts | null;
  by_distance: DistanceBand[];
  by_direction: DirectionBand[];
  info: ReportInfo;
}

/** While the bot moved one way: the share of the time it did, the time on it, the distance and the lag. */
export interface MotionBand {
  name: Direction;
  share: number;
  on: number | null;
  distance: number | null;
  lag: number | null;
}

/** A frame's offset from the bot's center line along its motion (positive: ahead) and across it, and its radius. */
export type AroundPoint = [along: number, across: number, radius: number];

/**
 * How the crosshair followed a moving bot (review.track_motion), from the camera's turn read in the video. reason:
 * why it was not measured. Distances in degrees; lag negative behind the bot.
 */
export interface Motion {
  reason?: string;
  seconds?: number;
  camera: number;
  lag?: number | null;
  lag_ms?: number | null;
  off_behind?: number | null;
  off_ahead?: number | null;
  off_side?: number | null;
  overshoots?: number | null;
  overshoot_dist?: number | null;
  overcorrect?: number | null;
  corrections?: number | null;
  swing_count?: number | null;
  reaction?: number | null;
  reversals?: number;
  reversal_overshoot?: number | null;
  reversal_overshoot_dist?: number | null;
  error_h?: number | null;
  error_v?: number | null;
  target_speed?: number | null;
  by_direction?: MotionBand[];
  /** Each frame of the motion measured: where the crosshair sat around the bot. */
  around?: AroundPoint[];
}

/** How much the accuracy would rise if one thing changed (review.what_if). gain: a share. */
export interface WhatIf {
  what: string;
  gain: number;
  how: string;
}

/**
 * The faint-target cut-off a tracking run was measured with: the user's offset, the score it cut at (null: the tracks
 * have no scores, so nothing was cut) and how many tracks it left out.
 */
export interface FaintCut {
  offset: number;
  cut: number | null;
  tracks: number;
}

/**
 * The user's faint-target cut-off for a recording (/api/faint, faint.json): on or off, and how far below the
 * recording's level a track may score (0.2 to 0.6). Once submitted: when, and how many labels it wrote (null while
 * they are being written).
 */
export interface FaintSetting {
  on: boolean;
  offset: number;
  submitted?: string;
  labels?: number | null;
}

/** The body of POST /api/faint. */
export interface FaintChoice {
  on: boolean;
  offset: number;
}

export interface TrackSummary {
  scenario: string;
  score: number | null;
  accuracy: number | null;
  on_target: number | null;
  on_all: number | null;
  error: number | null;
  lost: number | null;
  lost_cost: number | null;
  slip_cost: number | null;
  back: number | null;
  longest_off: number | null;
  bots: number;
  to_next: number | null;
  waiting: number | null;
  onto: number | null;
  switching: number | null;
  fps_avg: number | null;
  start: number | null;
  end: number | null;
  switches: Switch[];
  motion: Motion | null;
  what_if: WhatIf[];
  faint: FaintCut | null;
  info: ReportInfo;
}

/** What every report has. review_model: the model that made it (null: not recorded). */
export interface ReportBase {
  fps: number;
  geometry: Geometry;
  review_model: string | null;
  /** Kept by an older version of the review (the core's reports only): reviewing again gives what it lacks. */
  outdated?: boolean;
}

/** The user's marks for where a run starts and ends, in seconds; null where the review finds it. */
export interface RunMarks {
  start: number | null;
  end: number | null;
  length: number | null;
}

/**
 * A clicking run: one flick per kill. appeared: the frame each target first showed, by track id (joining a target's
 * track when the tracker lost and found it again).
 */
export interface ClickReport extends ReportBase {
  mode: 'click';
  summary: ClickSummary;
  flicks: Flick[];
  issues: Issue[];
  paths: Record<string, PathPoint[]>;
  appeared?: Record<string, number>;
  run?: RunMarks | null;
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
/** How far the view moved since the frame before, in degrees. */
export type ViewShift = [x: number, y: number];

export interface TrackFrame {
  i: number;
  t: TrackPoint[];
  shift?: ViewShift;
  a?: number[];
  wh?: TargetSize[];
  s?: number[];
}

/** Every target in every frame (/api/tracks). */
export interface Tracks {
  fps: number;
  frames: TrackFrame[];
  /** The share of the screen found fixed (crosshair, HUD), and what found the targets. */
  fixed?: number;
  detector?: string;
  /** The part of the video tracked, when only part of it was (the user's run window); the frames outside are empty. */
  window?: TimeWindow | null;
  /** The review's version (src/track.rs: `REVIEW_VERSION`); none from Python, or from before reviews kept it. */
  version?: number;
  /** The excluded areas it was tracked with, where the review keeps them (the browser); none: not known. */
  areas?: AreaBox[];
}

/** A part of a video, in seconds. */
export interface TimeWindow {
  start: number;
  end: number;
}

/** A part of the frame, as shares of its width and height: x0, y0, x1, y1. */
export type AreaRect = [x0: number, y0: number, x1: number, y1: number];

/**
 * An excluded area (/api/exclude): a part of the frame the review ignores (a webcam, an overlay), as shares of the
 * frame, and the id of its kind.
 */
export type AreaBox = [x0: number, y0: number, x1: number, y1: number, kind: string];

/** Where a recording's areas come from: saved for it, the last added recording's, or KovOBS's layout (the default). */
export type AreaSource = 'saved' | 'last upload' | 'kovobs';

/** A recording's excluded areas, and where they come from. */
export interface AreaSet {
  boxes: AreaBox[];
  source: AreaSource;
}

/**
 * A recording's areas as kept (POST /api/exclude), and the review the keeping started where the mode tracks again by
 * itself (the desktop app: the shown review was tracked with other areas); without it, the page decides.
 */
export interface KeptAreas extends AreaSet {
  job?: Job;
}

/** What an area can be (/api/area_kinds): its id never changes; its name and what it is can. */
export interface AreaKind {
  id: string;
  name: string;
  about: string;
}

/** A recording's excluded areas, with every kind an area can be. */
export interface RecordingAreas extends AreaSet {
  kinds: AreaKind[];
}

/** How many of the found areas were named from what the user taught, and how many by rules. */
export interface FoundBy {
  learned?: number;
  rule?: number;
}

/**
 * The area finder's proposal (/api/find_areas): the areas, how many examples and recordings it learned from, the
 * recording whose areas it copied (null: none, the areas were found in this one), and how the found ones were named.
 */
export interface FoundAreas {
  boxes: AreaBox[];
  examples: number;
  recordings: number;
  copied: string | null;
  by: FoundBy;
}

/** A new kind (id null), or a kind's new name and what it is. */
export interface KindEdit {
  id: string | null;
  name: string;
  about: string;
}

export type JobStage =
  | 'none'
  | 'starting'
  | 'looking'
  | 'tracking'
  | 'linking'
  | 'ffmpeg'
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

/**
 * How a recording's stats file came to it: the user picked one (picked) or uploaded one (upload) or said there is none
 * (none), the chosen file is missing (gone), it was uploaded with the VOD (beside), it was found by its name and time
 * (found), or there is none (missing).
 */
export type StatsHow = 'picked' | 'upload' | 'none' | 'gone' | 'beside' | 'found' | 'missing';

/** Where a chosen stats file is: KovaaK's stats folder, or uploaded from the page. */
export type StatsSource = 'kovaak' | 'upload';

/** A stats file to pair a recording with. off: seconds from the recording's time to the file's (negative: before). */
export interface StatsCandidate {
  name: string;
  scenario: string;
  stamp: string;
  off: number;
}

/** What the page shows of a stats file. accuracy is a share (0 to 1); stamp is when the run ended (its name). */
export interface StatsSummary {
  scenario: string | null;
  score: number | null;
  kills: number | null;
  accuracy: number | null;
  stamp: string | null;
}

/**
 * A recording's stats file, how it came to it, and stats files to pair it with, nearest in time first (/api/stats).
 * facts: what the file says, when it was read where the page runs.
 */
export interface StatsPairing {
  file: string | null;
  how: StatsHow;
  scenario: string;
  candidates: StatsCandidate[];
  facts?: StatsSummary;
}

/** The user's choice of stats file: a file, or none (file null). */
export interface StatsPick {
  file: string | null;
  source: StatsSource;
}

/** Back to finding the stats file by name and time. */
export interface StatsAuto {
  auto: true;
}

/** The body of POST /api/stats. */
export type StatsChoice = StatsPick | StatsAuto;

/** The answer to POST /api/stats: the job measuring the review again, and whether the recording has a stats file. */
export interface StatsChange {
  job: Job;
  stats: boolean;
}

/** The answer to an upload: the recording's id and the name saved; for a stats file, also the change it made. */
export interface Uploaded {
  id: string;
  saved: string;
  job?: Job;
  stats?: boolean;
}

/** The answer to marking a recording as another game, or as an aim trainer again (/api/not_aim). */
export interface NotAimMark {
  id: string;
  not_aim: boolean;
}

/**
 * One example the area finder learns from (a line of area_examples.jsonl, python/areas.py `learn`): the recording it
 * comes from, the area's features, and its type's id ("none": an area found there that the user removed).
 */
export interface AreaExample {
  rec: string;
  feat: number[];
  kind: string;
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
