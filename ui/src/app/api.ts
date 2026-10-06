/**
 * The review server's JSON API, typed. The JSON the core and the service write from Rust structs has its types made
 * from them (`bun run types`, into generated/), re-exported here under the UI's names; the rest is written by hand: the
 * answers the service builds without a struct (`json!`), the bodies the page sends, and the UI's own types.
 */

import { HttpErrorResponse } from '@angular/common/http';
import type { AreaBox } from './generated/area-box';
import type { AreaKind } from './generated/area-kind';
import type { ClickReport as CoreClickReport } from './generated/click-report';
import type { Kind } from './generated/kind';
import type { RunMarks } from './generated/run-marks';
import type { TimeWindow } from './generated/time-window';
import type { TrackFrame } from './generated/track-frame';
import type { TrackReport as CoreTrackReport } from './generated/track-report';

export type { AmmoRules } from './generated/ammo-rules';
export type { AroundPoint } from './generated/around-point';
export type { ClickWhatIf } from './generated/click-what-if';
export type { ClickWhatIfGroup } from './generated/click-what-if-group';
export type { CropAnswer } from './generated/crop-answer';
export type { CropAnswers } from './generated/crop-answers';
export type { CropBox } from './generated/crop-box';
export type { CropEntry } from './generated/crop-entry';
export type { CropPage } from './generated/crop-page';
export type { CropSet } from './generated/crop-set';
export type { CropVerdict } from './generated/crop-verdict';
export type { CrosshairSpot } from './generated/crosshair-spot';
export type { Direction } from './generated/direction';
export type { DirectionGroup as DirectionBand } from './generated/direction-group';
export type { DistanceGroup as DistanceBand } from './generated/distance-group';
export type { Facts as ScenarioInfo } from './generated/facts';
export type { FaceOffset } from './generated/face-offset';
export type { FaintCut } from './generated/faint-cut';
export type { FlickProfile } from './generated/flick-profile';
export type { Geometry } from './generated/geometry';
export type { Issue } from './generated/issue';
export type { KillParts } from './generated/kill-parts';
export type { KillSource as Source } from './generated/kill-source';
export type { LinkFormat } from './generated/link-format';
export type { LinkInfo } from './generated/link-info';
export type { MatchInfo } from './generated/match-info';
export type { Measure as Flick } from './generated/measure';
export type { Motion } from './generated/motion';
export type { MotionDirection as MotionBand } from './generated/motion-direction';
export type { PathPoint } from './generated/path-point';
export type { Reloads } from './generated/reloads';
export type { Scene } from './generated/scene';
export type { SceneView } from './generated/scene-view';
export type { SecondShares } from './generated/second-shares';
export type { Shape } from './generated/shape';
export type { ShapeKind } from './generated/shape-kind';
export type { ShapeRole } from './generated/shape-role';
export type { Solid } from './generated/solid';
export type { SpeedCurve } from './generated/speed-curve';
export type { Summary as ClickSummary } from './generated/summary';
export type { Switch } from './generated/switch';
export type { TargetOffset } from './generated/target-offset';
export type { TargetSize } from './generated/target-size';
export type { TargetView } from './generated/target-view';
export type { TrackInfo } from './generated/track-info';
export type { TrackPoint } from './generated/track-point';
export type { TrackSummary } from './generated/track-summary';
export type { TurnBack } from './generated/turn-back';
export type { ViewShift } from './generated/view-shift';
export type { WhatIf } from './generated/what-if';
export type { AreaBox, AreaKind, Kind, RunMarks, TimeWindow, TrackFrame };

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
  /** The devices the user can choose between (the browser's, or the ones the review server can run). */
  devices?: Device[];
  /** How many frames the detector takes in one go on this device, and the choices. */
  batch?: number;
  batches?: number[];
  speed: string;
  checked_on: string;
  checks: Check[];
  models: Model[];
  /** Why a model that is not available cannot run here; without it, it needs the GPU. */
  unavailable?: string;
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
  /**
   * From the quick list (/api/vods?quick=1): only what the file's name gives, its kind, stats file and review not
   * looked at yet (they read as none until the whole list is in).
   */
  quick?: boolean;
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

/**
 * What the service and the browser give with the core's report: the model that made it (null: not recorded), and the
 * user's run marks (the core passes them through as given).
 */
export interface ReportBase {
  review_model: string | null;
  run: RunMarks | null;
}

/** A clicking run: one flick per kill (src/review.rs: `Report`). */
export type ClickReport = Omit<CoreClickReport, 'run'> & ReportBase;

/** A tracking run: time on the bot, measured frame by frame (src/review.rs: `TrackReport`). */
export type TrackReport = Omit<CoreTrackReport, 'run'> & ReportBase;

export type Report = ClickReport | TrackReport;

/** A clicking run's report (click or hold mode: every run but a tracking one), as the old page read them. */
export function isClickReport(report: Report | null | undefined): report is ClickReport {
  return !!report && report.mode !== 'track';
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

/** A part of the frame, as shares of its width and height: x0, y0, x1, y1. */
export type AreaRect = [x0: number, y0: number, x1: number, y1: number];

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
  | 'downloading'
  | 'yt-dlp'
  | 'done'
  | 'error';

/**
 * A review in progress, or its end (/api/job). A link's download is a job too (link): its stage is downloading
 * (megabytes done of total), or ffmpeg or yt-dlp while the server fetches them; it is gone (none) once the video is in.
 */
export interface Job {
  stage: JobStage;
  done?: number;
  total?: number;
  seconds?: number;
  error?: string;
  /** The device the detector runs on, once it has loaded ("DirectML", "DirectML and CPU"); the browser's job has none. */
  device?: string;
  /** The model the service's review runs with; the browser's job, and none (no job), have none. */
  model?: string;
  link?: boolean;
}

/** The answer to POST /api/link: the new recording's id, its file name, and its row while it downloads. */
export interface LinkAdded {
  id: string;
  saved: string;
  title: string;
  recording: Recording;
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
export function errorMessage(error: unknown): string {
  if (error instanceof HttpErrorResponse)
    return (error.error as ApiError | null)?.error ?? error.message;
  return error instanceof Error ? error.message : String(error);
}
