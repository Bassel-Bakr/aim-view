import { Injectable } from '@angular/core';
import { Report, RunMarks, ScenarioInfo, Tracks } from '../../api';
import { Core } from './core';
import { CameraReading } from './review-messages';

/**
 * What the core reviews a run from (src/review.rs: `ReviewRequest`): its tracks, its video's and stats file's names,
 * the stats file's text and the user's run marks; for a tracking run also the scenario's time limit and the video's
 * readings.
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

  /** A run's report from its tracks and stats file: src/review.rs. */
  async report(request: ReportRequest): Promise<Report> {
    const text = await this.call((c, p, n) => c.x.review_report(p, n), JSON.stringify(request));
    const outcome = JSON.parse(text) as ReportOutcome;
    if ('error' in outcome) throw new Error(outcome.error);
    return outcome.report;
  }
}
