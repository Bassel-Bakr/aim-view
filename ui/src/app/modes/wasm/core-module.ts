import { Injectable } from '@angular/core';
import { Report, RunMarks, ScenarioInfo, TrackFrame, Tracks } from '../../api';
import { Core } from './core';
import { CameraReading, HudReading, RunPart, VideoReadings } from './review-messages';

/**
 * What the core reviews a run from (src/review.rs: `ReviewRequest`): its tracks, its video's and stats file's names,
 * the stats file's text (both '' for a run without one), the user's run marks and what the HUD read (null: nothing);
 * for a tracking run also the scenario's time limit and the video's readings.
 */
export interface ReportRequest {
  tracks: Tracks;
  statsText: string;
  video: string;
  stats: string;
  run: RunMarks | null;
  tracking: boolean;
  limit: number | null;
  camera: CameraReading[];
  countdown: boolean[];
  hud: HudReading | null;
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
 * A review's runs joined: the tracks' frames, linked, the video's readings, what the HUD read (null: nothing), and the
 * review's version (src/track.rs: `REVIEW_VERSION`).
 */
export interface JoinedRuns {
  frames: TrackFrame[];
  readings: VideoReadings;
  hud: HudReading | null;
  version: number;
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

  /** What a scenario file says (its text, at least up to "[Map Data]"): src/scenario.rs. */
  async scenarioFacts(text: string): Promise<ScenarioInfo> {
    return JSON.parse(await this.call((c, p, n) => c.x.scenario_facts(p, n), text)) as ScenarioInfo;
  }

  /**
   * The parts of a recording's runs (one per review worker: split-runs.ts), joined in order (src/wasm.rs:
   * tracker_add_part, camera_add_part, hud_add_part): the frames linked, then the camera's readings, which need the
   * tracks, and the HUD's. `cap`: the scenario's target count, 0 for none.
   */
  async joinRuns(parts: RunPart[], cap: number): Promise<JoinedRuns> {
    const core = await this.load();
    const withText = (text: string, use: (ptr: number, len: number) => number) => {
      const bytes = new TextEncoder().encode(text);
      const block = core.reserve(bytes.length);
      core.bytes(block).set(bytes);
      const out = use(block.ptr, bytes.length);
      core.free(block);
      return out;
    };
    const tracker = core.x.tracker_new_kovobs(cap);
    const fixed = core.reserve(parts[0].fixed.length);
    core.bytes(fixed).set(parts[0].fixed);
    const camera = core.x.camera_new(fixed.ptr);
    core.free(fixed);
    const { width, height, full } = parts[0].format;
    const hud = core.x.hud_new(width, height, full);
    let joined = true;
    let cameraFrames = 0;
    let hudFrames = 0;
    for (const p of parts) {
      joined &&=
        withText(p.track, (ptr, len) => core.x.tracker_add_part(tracker, ptr, len)) === p.frames;
      cameraFrames = withText(p.camera, (ptr, len) => core.x.camera_add_part(camera, ptr, len));
      hudFrames = withText(p.hud, (ptr, len) => core.x.hud_add_part(hud, ptr, len));
    }
    const framesText = core.takeText(core.x.tracker_finish(tracker));
    const readingsText = core.takeText(
      withText(framesText, (ptr, len) => core.x.camera_finish(camera, ptr, len)),
    );
    const hudText = core.takeText(core.x.hud_finish(hud));
    const frames = JSON.parse(framesText) as TrackFrame[];
    // a review from part way in has empty frames before its first, in the tracks and both watches' readings alike
    if (!joined || cameraFrames !== frames.length || hudFrames !== frames.length) {
      throw new Error("The review's runs do not join up");
    }
    return {
      frames,
      readings: JSON.parse(readingsText) as VideoReadings,
      hud: JSON.parse(hudText) as HudReading | null,
      version: core.x.review_version(),
    };
  }

  /** A run's report from its tracks and stats file, or without one from the HUD or the video alone: src/review.rs. */
  async report(request: ReportRequest): Promise<Report> {
    const text = await this.call((c, p, n) => c.x.review_report(p, n), JSON.stringify(request));
    const outcome = JSON.parse(text) as ReportOutcome;
    if ('error' in outcome) throw new Error(outcome.error);
    return outcome.report;
  }
}
