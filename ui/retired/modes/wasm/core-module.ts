import { Injectable } from '@angular/core';
import {
  AmmoRules,
  AreaBox,
  AreaExample,
  AreaKind,
  AreaRect,
  FaintChoice,
  FoundAreas,
  Report,
  RunMarks,
  ScenarioInfo,
  TrackFrame,
  Tracks,
} from '../../api';
import { CutoffRow } from '../web-files/cutoff-labels';
import { Core } from './core';
import { LabelledRecording } from '../web-files/saved-areas';
import { FoundArea } from './area-finder-messages';
import { CameraReading, HudReading, RunPart, VideoReadings } from './review-messages';

/**
 * What the area finder proposes from (src/areas.rs: find_json): the areas found in the recording, the examples it
 * learned, the recordings the user saved areas for (to copy one with the same layout; none for Detect fresh), and the
 * kinds (they turn the proposal's kinds into ids).
 */
export interface AreasFindRequest {
  found: FoundArea[];
  examples: readonly AreaExample[];
  labelled: LabelledRecording[];
  kinds: AreaKind[];
}

/**
 * What the area finder learns from (src/areas.rs: learn_json): the recording's name in the examples, the areas found
 * in it and their maps (FinderResult), the areas the user saved, and the kinds.
 */
export interface AreasLearnRequest {
  rec: string;
  found: FoundArea[];
  maps: unknown;
  saved: AreaBox[];
  kinds: AreaKind[];
}

/** learn_json's answer: the examples as the lines of area_examples.jsonl, and how many were added. */
interface AreasLearned {
  examples: string;
  added: number;
}

/** A finder call the core refused, and why. */
interface CoreRefusal {
  error: string;
}

/**
 * What the core reviews a run from (src/review.rs: `ReviewRequest`): its tracks, its video's and stats file's names,
 * the stats file's text (both '' for a run without one), the user's run marks and what the HUD read (null: nothing);
 * for a clicking run the ammo rules of the scenario's weapon (null: its magazine never runs out, or not known); for a
 * tracking run also the scenario's time limit, the video's readings and the user's faint-target cut-off (null:
 * none), whose cut its measures leave out.
 */
export interface ReportRequest {
  tracks: Tracks;
  statsText: string;
  video: string;
  stats: string;
  run: RunMarks | null;
  tracking: boolean;
  limit: number | null;
  reload: AmmoRules | null;
  camera: CameraReading[];
  countdown: boolean[];
  hud: HudReading | null;
  faint: FaintChoice | null;
}

/**
 * What the labels of a submitted cut-off are made from (src/faint.rs: `CutoffRequest`): the tracks' frames, the
 * recording's name, the run's first and last frames (null: not known, no labels), the excluded areas (null: KovOBS's
 * layout), the cut-off's offset, and how near the crosshair a score is not counted (a tracking run 0, a clicking run 2
 * degrees).
 */
export interface CutoffRequest {
  frames: TrackFrame[];
  video: string;
  start: number | null;
  end: number | null;
  exclude: AreaRect[] | null;
  offset: number;
  near: number;
}

/**
 * A label's crop (src/faint.rs: `CutoffCrop`): its frame and corner (pixels at 1280 x 720), the boxes it keeps (crop
 * pixels: center and size, written as float32) and its row in checked.jsonl.
 */
export interface CutoffCrop {
  frame: number;
  x0: number;
  y0: number;
  boxes: number[][];
  row: CutoffRow;
}

/** The core's report. */
export interface ReportMade {
  report: Report;
}

/** Why the core gave no report. */
export interface ReportRefused {
  error: string;
}

/** The core's answer: the report, or why there is none. */
export type ReportOutcome = ReportMade | ReportRefused;

/**
 * A review's runs joined (src/session.rs: `Joined`): the tracks (tracks.json), the video's readings and what the HUD
 * read (null: nothing).
 */
export interface JoinedReview {
  tracks: Tracks;
  readings: VideoReadings;
  hud: HudReading | null;
}

/** A core export that takes text and hands text back. */
type TextCall = (core: Core, ptr: number, len: number) => number;

/** The review core on the page itself (the tracking runs in a worker; this is for what is quick to work out). */
@Injectable({ providedIn: 'root' })
export class CoreModule {
  private core: Promise<Core> | null = null;

  private load(): Promise<Core> {
    this.core ??= Core.load(new URL('core/aimview.wasm', document.baseURI).href);
    return this.core;
  }

  private async call(fn: TextCall, text: string): Promise<string> {
    const core = await this.load();
    const bytes = new TextEncoder().encode(text);
    const block = core.reserve(bytes.length);
    core.bytes(block).set(bytes);
    const out = fn(core, block.ptr, bytes.length);
    core.free(block);
    return core.takeText(out);
  }

  /** The area finder's proposal (src/areas.rs: find_json). */
  async areasFind(request: AreasFindRequest): Promise<FoundAreas> {
    const text = await this.call((c, p, n) => c.x.areas_find(p, n), JSON.stringify(request));
    const out = JSON.parse(text) as FoundAreas | CoreRefusal;
    if ('error' in out) throw new Error(out.error);
    return out;
  }

  /** The examples the area finder learns from a recording's saved areas (src/areas.rs: learn_json). */
  async areasLearn(request: AreasLearnRequest): Promise<AreaExample[]> {
    const text = await this.call((c, p, n) => c.x.areas_learn(p, n), JSON.stringify(request));
    const out = JSON.parse(text) as AreasLearned | CoreRefusal;
    if ('error' in out) throw new Error(out.error);
    return out.examples
      .split(/\r?\n/)
      .filter((line) => line.trim())
      .map((line) => JSON.parse(line) as AreaExample);
  }

  /** What a scenario file says (its text, at least up to "[Map Data]"): src/scenario.rs. */
  async scenarioFacts(text: string): Promise<ScenarioInfo> {
    return JSON.parse(await this.call((c, p, n) => c.x.scenario_facts(p, n), text)) as ScenarioInfo;
  }

  /** A raw mouse log read (src/mouse.rs): its bytes and the request (MouseReadRequest); the answer as JSON. */
  async mouseRead(log: Uint8Array, request: string): Promise<string> {
    const core = await this.load();
    const block = core.reserve(log.length);
    core.bytes(block).set(log);
    const out = await this.call((c, p, n) => c.x.mouse_read(block.ptr, block.len, p, n), request);
    core.free(block);
    return out;
  }

  /**
   * A review's runs joined in order (src/session.rs: `Joining`): the parts each review worker gave, the first's setup
   * and fixed map (every run's are the same). `detector`: the detector that ran, as tracks.json names it.
   */
  async joinReview(parts: RunPart[], detector: string): Promise<JoinedReview> {
    const core = await this.load();
    const review = core.review(parts[0].setup);
    const fixed = core.reserve(parts[0].fixed.length);
    core.bytes(fixed).set(parts[0].fixed);
    const joining = core.x.review_joining(review, fixed.ptr);
    core.free(fixed);
    core.x.review_free(review);
    for (const p of parts) {
      core.textIn(p.track, (track, trackLen) =>
        core.textIn(p.watch, (watch, watchLen) =>
          core.x.joining_add(joining, track, trackLen, watch, watchLen),
        ),
      );
    }
    const joined = core.textIn(detector, (ptr, len) => core.x.joining_finish(joining, ptr, len));
    return JSON.parse(core.takeOutcome(joined)) as JoinedReview;
  }

  /** A run's report from its tracks and stats file, or without one from the HUD or the video alone: src/review.rs. */
  /** The crops a submitted cut-off's labels take, as Python's hand_crops.py picks them: src/faint.rs. */
  async cutoffCrops(request: CutoffRequest): Promise<CutoffCrop[]> {
    const text = await this.call((c, p, n) => c.x.cutoff_crops(p, n), JSON.stringify(request));
    const out = JSON.parse(text) as CutoffCrop[] | ReportRefused;
    if ('error' in out) throw new Error(out.error);
    return out;
  }

  async report(request: ReportRequest): Promise<Report> {
    const text = await this.call((c, p, n) => c.x.review_report(p, n), JSON.stringify(request));
    const outcome = JSON.parse(text) as ReportOutcome;
    if ('error' in outcome) throw new Error(outcome.error);
    return outcome.report;
  }
}
