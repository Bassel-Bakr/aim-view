import { Injectable } from '@angular/core';
import { ClickReport, RunMarks, ScenarioInfo, Tracks } from '../../api';
import { Core } from './core';

/** What the core reviews a clicking run from (src/review.rs: `ClickRequest`). */
export interface ClickRequest {
  tracks: Tracks;
  statsText: string;
  video: string;
  stats: string;
  run: RunMarks | null;
}

/** The core's report. */
export interface ClickReviewed {
  report: ClickReport;
}

/** Why the core gave no report. */
export interface ClickRefused {
  error: string;
}

/** The core's answer: the report, or why there is none. */
export type ClickOutcome = ClickReviewed | ClickRefused;

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

  /** A clicking run's report from its tracks and stats file: src/review.rs. */
  async reviewClicks(request: ClickRequest): Promise<ClickReport> {
    const text = await this.call((c, p, n) => c.x.review_clicks(p, n), JSON.stringify(request));
    const outcome = JSON.parse(text) as ClickOutcome;
    if ('error' in outcome) throw new Error(outcome.error);
    return outcome.report;
  }
}
