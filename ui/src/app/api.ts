/**
 * The review server's JSON API, typed. The JSON the core and the service write from Rust structs has its types made
 * from them (`bun run types`, into generated/), re-exported here under the UI's names; the rest is written by hand: the
 * answers the service builds without a struct (`json!`), the bodies the page sends, and the UI's own types.
 * In: the service's answers (service/src/api.rs), the same in every mode. Out: the platform
 * contracts, the modes and the features, which import these types from here, not from generated/.
 */

import { HttpErrorResponse } from '@angular/common/http';
import type { AreaBox } from './generated/area-box';
import type { AreaKind } from './generated/area-kind';
import type { ClickReport as CoreClickReport } from './generated/click-report';
import type { HitboxKind } from './generated/hitbox-kind';
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
export type { CropVertex } from './generated/crop-vertex';
export type { CropEntry } from './generated/crop-entry';
export type { CropPage } from './generated/crop-page';
export type { CropSet } from './generated/crop-set';
export type { CropVerdict } from './generated/crop-verdict';
export type { Hitbox } from './generated/hitbox';
export type { HitboxKind } from './generated/hitbox-kind';
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
export type { Kind as RunKind } from './generated/kind';
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

/**
 * Milliseconds per 1280 x 720 frame on each runtime, as models.json gives them (python/model/
 * MODEL_STATUS.md, "Deployment").
 */
export interface ModelSpeed {
  /** PyTorch on the GPU, in batches of 16. */
  gpu: number;
  /** ONNX Runtime on the CPU (fp32, 4 threads). */
  cpu: number;
  /** onnxruntime-web on WebAssembly, in the browser. */
  browser: number;
}

/** A model's result on one check: [kills matched, flicks measured], or for tracking [mean, mean size]. */
export type CheckResult = [first: number, second: number];

/** The checks every model is measured on: static runs, moving runs, uploads, tracking runs. */
export type CheckKey = 'static' | 'moving' | 'uploads' | 'tracking';

/** A model's result on each check it was measured on. */
export type ModelChecks = Partial<Record<CheckKey, CheckResult>>;

/** A detector model the user can pick, as models.json describes it (/api/models). */
export interface Model {
  /** Its id: its export is detector_<name>_u8in.onnx. */
  name: string;
  /** The name the page shows (the id when models.json gives none). */
  label: string;
  /** Whether it can run here; the service lists only models whose export it has. */
  available: boolean;
  /** Whether it is models.json's default, the one new reviews use until the user picks another. */
  default?: boolean;
  /** Listed under the table of models instead of in it. */
  older?: boolean;
  /**
   * When it passed the acceptance gate (python/model/accept.py), and its report: "2026-10-06: <report> ..."; absent
   * for a model that did not.
   */
  accepted?: string;
  /** How many parameters it has. */
  params?: number;
  /** Its fp32 ONNX file's size, in kilobytes; null when it has only a PyTorch file. */
  kb?: number | null;
  /** What it was trained on, in words. */
  trained?: string;
  /** Where it does best, in words. */
  best?: string;
  /** Where it does worse, in words. */
  weak?: string;
  /** Its speed on each runtime. */
  speed_ms?: ModelSpeed;
  /** Its result on each check. */
  checks?: ModelChecks;
}

/** One of the checks every model was measured on. */
export interface Check {
  /** Which check it is. */
  key: CheckKey;
  /** Its name on the page ("Static runs"). */
  name: string;
  /** What its two numbers are ("kills matched, flicks measured"). */
  what: string;
  /** How many kills the check has, for a check that counts them. */
  of?: number;
}

/**
 * Where the detector runs: the server's GPU (cuda) or CPU, in the browser on the CPU (wasm) or the GPU (webgpu), or
 * in the desktop app on the GPU (directml).
 */
export type Device = 'cuda' | 'cpu' | 'wasm' | 'webgpu' | 'directml';

/** The models to pick from, the one picked, and where and how they run (/api/models). */
export interface ModelList {
  /** The model new reviews use. */
  chosen: string;
  /** Where the detector runs now. */
  device: Device;
  /** The devices the user can choose between (the browser's, or the ones the review server can run). */
  devices?: Device[];
  /** How many frames the detector takes in one go on this device. */
  batch?: number;
  /** The frame counts the user can pick for `batch`. */
  batches?: number[];
  /** How the speeds were measured: the machine and each runtime's settings, in words. */
  speed: string;
  /** Which recordings the checks ran on, in words. */
  checked_on: string;
  /** The checks every model was measured on, in the page's order. */
  checks: Check[];
  /** The models, in models.json's order. */
  models: Model[];
  /** Why a model that is not available cannot run here; without it, it needs the GPU. */
  unavailable?: string;
}

/** A recording in the list (/api/vods). kind is null for a scenario the game no longer has. */
export interface Recording {
  /** Its path in the VODs folder ("<folder>/<file>"), or "uploads/<file>" for an upload. */
  id: string;
  /** The scenario from its file name, or a link's title, or the file's own name. */
  scenario: string;
  /** The run's kind: the user's choice (kind_pick), else its scenario's, from KovaaK's scenario files. */
  kind: Kind | null;
  /** The kind the user chose for it (kind.json); null or absent when it follows its scenario. */
  kind_pick?: Kind | null;
  /** The bots' hitbox shape the user chose for it (hitbox.json); null or absent when it follows its scenario. */
  hitbox_pick?: HitboxKind | null;
  /** The score from its file name; null when the name gives none. */
  score: number | null;
  /** When it was recorded, local time, as KovOBS names it ("2026.10.02-12.34.56"). */
  stamp: string;
  /** When the file last changed, in seconds since 1970 (0 in the quick list). */
  mtime: number;
  /** The file's size, in bytes (0 in the quick list). */
  size: number;
  /** Whether it has a stats file, picked or found. */
  stats: boolean;
  /** Whether it has a review. */
  analysed: boolean;
  /** Whether the user marked it as another game, not an aim trainer. */
  not_aim: boolean;
  /** Whether it was added from this computer or a link, not found in the VODs folder. */
  uploaded?: boolean;
  /** Opened from this computer: it stays in this browser and is gone when the page closes. */
  local?: boolean;
  /**
   * From the quick list (/api/vods?quick=1): only what the file's name gives, its kind, stats file and review not
   * looked at yet (they read as none until the whole list is in).
   */
  quick?: boolean;
}

/** A recording's kind after the user chose one (POST /api/kind): its row's `kind` and `kind_pick`. */
export interface KindChange {
  /** The run's kind now: the choice, else its scenario's; null when neither is known. */
  kind: Kind | null;
  /** The user's choice; null when it follows its scenario. */
  kind_pick: Kind | null;
}

/** A recording's hitbox after the user chose one (POST /api/hitbox): its row's `hitbox_pick`. */
export interface HitboxChange {
  /** The user's choice; null when it follows its scenario. */
  hitbox_pick: HitboxKind | null;
}

/**
 * The user's faint-target cut-off for a recording (/api/faint, faint.json): on or off, and how far below the
 * recording's level a track may score (0.2 to 0.6). Once submitted: when, and how many labels it wrote (null while
 * they are being written).
 */
export interface FaintSetting {
  /** Whether the cut-off leaves faint tracks out of the review. */
  on: boolean;
  /** How far below the recording's level a track may score and still count (0.2 to 0.6). */
  offset: number;
  /** When the user submitted it, as an ISO time. */
  submitted?: string;
  /** How many labels the submit wrote; null while they are being written. */
  labels?: number | null;
}

/** The body of POST /api/faint. */
export interface FaintChoice {
  /** Whether the cut-off is on. */
  on: boolean;
  /** How far below the recording's level a track may score (0.2 to 0.6). */
  offset: number;
}

/**
 * What the service and the browser give with the core's report: the model that made it (null: not recorded), and the
 * user's run marks (the core passes them through as given).
 */
export interface ReportBase {
  /** The model that made the review; null when it was not recorded. */
  review_model: string | null;
  /** The user's run marks (run.json); null when there are none. */
  run: RunMarks | null;
}

/** A clicking run: one flick per kill (src/review.rs: `Report`). */
export type ClickReport = Omit<CoreClickReport, 'run'> & ReportBase;

/** A tracking run: time on the bot, measured frame by frame (src/review.rs: `TrackReport`). */
export type TrackReport = Omit<CoreTrackReport, 'run'> & ReportBase;

/** A run's report (/api/report): a clicking run's or a tracking run's. */
export type Report = ClickReport | TrackReport;

/** A clicking run's report (click or hold mode: every run but a tracking one), as the old page read them. */
export function isClickReport(report: Report | null | undefined): report is ClickReport {
  return !!report && report.mode !== 'track';
}

/** Every target in every frame (/api/tracks). */
export interface Tracks {
  /** The video's frames a second. */
  fps: number;
  /** Each frame's targets and view shift, one entry per frame of the video. */
  frames: TrackFrame[];
  /** The share of the screen found fixed (crosshair, HUD). */
  fixed?: number;
  /** The model that found the targets. */
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
  /** The areas, each a box in shares of the frame with its type. */
  boxes: AreaBox[];
  /** Where they come from. */
  source: AreaSource;
}

/**
 * A recording's areas as kept (POST /api/exclude), and the review the keeping started where the mode tracks again by
 * itself (the desktop app: the shown review was tracked with other areas); without it, the page decides.
 */
export interface KeptAreas extends AreaSet {
  /** The review the keeping started, where the mode tracks again by itself. */
  job?: Job;
}

/** A recording's excluded areas, with every kind an area can be. */
export interface RecordingAreas extends AreaSet {
  /** Every type an area can have, the user's own included (area_kinds.json). */
  kinds: AreaKind[];
}

/** How many of the found areas were named from what the user taught, and how many by rules. */
export interface FoundBy {
  /** Areas whose type came from the user's examples. */
  learned?: number;
  /** Areas whose type came from the finder's rules. */
  rule?: number;
}

/**
 * The area finder's proposal (/api/find_areas): the areas, how many examples and recordings it learned from, the
 * recording whose areas it copied (null: none, the areas were found in this one), and how the found ones were named.
 */
export interface FoundAreas {
  /** The areas it proposes. */
  boxes: AreaBox[];
  /** How many saved examples it learned from. */
  examples: number;
  /** How many recordings those examples come from. */
  recordings: number;
  /** The recording whose areas it copied; null when it found them in this one. */
  copied: string | null;
  /** How the found areas got their types. */
  by: FoundBy;
}

/** A new kind (id null), or a kind's new name and what it is. */
export interface KindEdit {
  /** The kind to change; null for a new one. */
  id: string | null;
  /** Its name on the page. */
  name: string;
  /** What areas of this kind are, in the user's words. */
  about: string;
}

/**
 * What a job is doing (the service's `Job.stage`): a review's steps, a link's download, or its
 * end. run/job-progress.ts gives each one's words on the page.
 */
export type JobStage =
  | 'none'
  | 'starting'
  | 'looking'
  | 'tracking'
  | 'linking'
  | 'checking'
  | 'ffmpeg'
  | 'reading the HUD'
  | 'camera'
  | 'measuring'
  | 'downloading'
  | 'yt-dlp'
  | 'done'
  | 'error'
  | 'cancelled';

/** The error a review or a download the user cancelled stops with (the service's job stage too). */
export const CANCELLED = 'cancelled';

/**
 * A review in progress, or its end (/api/job). The user can cancel one (/api/cancel): it ends as cancelled, keeping
 * nothing. A link's download is a job too (link): its stage is downloading
 * (megabytes done of total), or ffmpeg or yt-dlp while the server fetches them; it is gone (none) once the video is in.
 */
export interface Job {
  /** What it is doing, or how it ended. */
  stage: JobStage;
  /** How much of the stage is done: frames, or megabytes for a download. */
  done?: number;
  /** How much the stage has to do, in the same unit as `done`. */
  total?: number;
  /** How long the review took, in seconds, once it is done. */
  seconds?: number;
  /** Why it failed, when the stage is error or cancelled. */
  error?: string;
  /** The device the detector runs on, once it has loaded ("DirectML", "DirectML and CPU"); the browser's job has none. */
  device?: string;
  /** The model the service's review runs with; the browser's job, and none (no job), have none. */
  model?: string;
  /** Whether it is a link's download, not a review. */
  link?: boolean;
}

/** The answer to POST /api/link: the new recording's id, its file name, and its row while it downloads. */
export interface LinkAdded {
  /** The new recording's id. */
  id: string;
  /** The file name the video is saved under. */
  saved: string;
  /** The video's title, from the link. */
  title: string;
  /** Its row in the recordings list while it downloads. */
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
  /** The file's name. */
  name: string;
  /** The scenario its name gives. */
  scenario: string;
  /** When its run ended, from its name. */
  stamp: string;
  /** Seconds from the recording's time to the file's (negative: before). */
  off: number;
}

/** What the page shows of a stats file. accuracy is a share (0 to 1); stamp is when the run ended (its name). */
export interface StatsSummary {
  /** The scenario it is for. */
  scenario: string | null;
  /** The run's score. */
  score: number | null;
  /** How many kills the run had. */
  kills: number | null;
  /** Hits over shots, a share from 0 to 1. */
  accuracy: number | null;
  /** When the run ended, from the file's name. */
  stamp: string | null;
}

/**
 * A recording's stats file, how it came to it, and stats files to pair it with, nearest in time first (/api/stats).
 * facts: what the file says, when it was read where the page runs.
 */
export interface StatsPairing {
  /** The paired stats file's name; null when it has none. */
  file: string | null;
  /** How the file came to it. */
  how: StatsHow;
  /** The recording's scenario, which the candidates are for. */
  scenario: string;
  /** Stats files to pair it with, nearest in time first. */
  candidates: StatsCandidate[];
  /** What the paired file says, where the page reads it. */
  facts?: StatsSummary;
}

/** The user's choice of stats file: a file, or none (file null). */
export interface StatsPick {
  /** The file's name; null to say the recording has none. */
  file: string | null;
  /** Where the file is. */
  source: StatsSource;
}

/** Back to finding the stats file by name and time. */
export interface StatsAuto {
  /** Always true: find the file again. */
  auto: true;
}

/** The body of POST /api/stats. */
export type StatsChoice = StatsPick | StatsAuto;

/** The answer to POST /api/stats: the job measuring the review again, and whether the recording has a stats file. */
export interface StatsChange {
  /** The job measuring the review again with the new stats file. */
  job: Job;
  /** Whether the recording has a stats file now. */
  stats: boolean;
}

/** The answer to an upload: the recording's id and the name saved; for a stats file, also the change it made. */
export interface Uploaded {
  /** The recording's id: the new video's, or the one the stats file was added to. */
  id: string;
  /** The file name it was saved under ("run (2).mp4" when the name was taken). */
  saved: string;
  /** For a stats file: the job measuring the review again. */
  job?: Job;
  /** For a stats file: whether the recording has one now. */
  stats?: boolean;
}

/** The answer to marking a recording as another game, or as an aim trainer again (/api/not_aim). */
export interface NotAimMark {
  /** The recording's id. */
  id: string;
  /** Whether it is marked as another game now. */
  not_aim: boolean;
}

/**
 * One example the area finder learns from (a line of area_examples.jsonl, python/areas.py `learn`): the recording it
 * comes from, the area's features, and its type's id ("none": an area found there that the user removed).
 */
export interface AreaExample {
  /** The id of the recording it comes from. */
  rec: string;
  /** The area's features, as src/areas.rs measures them. */
  feat: number[];
  /** The id of the area's type, or "none". */
  kind: string;
}

/** The body of an error response. */
export interface ApiError {
  /** What went wrong, in the server's words. */
  error?: string;
}

/** What went wrong with a request: the server's own message when it sent one ({error}), else the request's. */
export function errorMessage(error: unknown): string {
  if (error instanceof HttpErrorResponse)
    return (error.error as ApiError | null)?.error ?? error.message;
  return error instanceof Error ? error.message : String(error);
}
