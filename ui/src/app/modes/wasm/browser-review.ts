import { computed, inject, Injectable, resource, ResourceRef, signal } from '@angular/core';
import { Job, Report, Tracks } from '../../api';
import { ModelCatalog } from '../../platform/model-catalog';
import { ReviewEngine } from '../../platform/review-engine';
import { LocalFiles, localRecording } from '../web-files/local-files';
import { ScenarioFacts } from '../web-files/scenario-facts';
import { StatsCsv } from '../web-files/stats-csv';
import { CoreModule } from './core-module';
import { ReviewMessage, ReviewRequest } from './review-messages';

const NOT_OPEN = 'The recording is not open in this browser.';
const TRACKING = 'Tracking runs are not reviewed in the browser yet.';
const NO_STATS =
  'Pair this run with its stats file to review it in the browser. Runs without one (read from the HUD or the ' +
  'video alone) are not reviewed here yet.';
const DEFAULT_MODEL = 'full_v3';

/** A recording's tracks, as the worker found them, and the model that found them. */
export interface FoundTracks {
  tracks: Tracks;
  model: string;
}

/** A recording's run in the worker: where it stands. */
export interface BrowserRun {
  job: Job;
}

/** What a report is worked out from: the recording, its video's name, its stats file and its tracks (when found). */
export interface ReportParams {
  id: string;
  video: string;
  stats: StatsCsv | null;
  found: FoundTracks | undefined;
}

function sameParams(a: ReportParams | undefined, b: ReportParams | undefined): boolean {
  return a?.id === b?.id && a?.stats === b?.stats && a?.found === b?.found;
}

/**
 * The review in the browser: the detector (onnxruntime-web) and the review core (Rust, as WebAssembly). A worker
 * (review.worker.ts) finds the tracks; the core on the page measures them against the stats file, so a stats file
 * paired later gives a report without finding the tracks again. Clicking runs with a stats file so far.
 */
@Injectable({ providedIn: 'root' })
export class BrowserReview implements ReviewEngine {
  private readonly local = inject(LocalFiles);
  private readonly scenarios = inject(ScenarioFacts);
  private readonly models = inject(ModelCatalog);
  private readonly core = inject(CoreModule);
  private readonly runs = new Map<string, BrowserRun>();
  private readonly found = signal<ReadonlyMap<string, FoundTracks>>(new Map());

  unavailable(id: string): string | null {
    const f = this.local.find(id);
    if (!f) return NOT_OPEN;
    if (this.scenarios.kind(localRecording(f).scenario) === 'tracking') return TRACKING;
    return f.stats ? null : NO_STATS;
  }

  report(id: () => string | undefined): ResourceRef<Report | null | undefined> {
    const params = computed<ReportParams | undefined>(
      () => {
        const at = id();
        const f = at === undefined ? null : this.local.find(at);
        if (!f) return undefined;
        return { id: f.id, video: f.file.name, stats: f.stats, found: this.found().get(f.id) };
      },
      { equal: sameParams },
    );
    return resource({
      params,
      loader: async ({ params: p }) => {
        if (!p.stats || !p.found) return null;
        const report = await this.core.reviewClicks({
          tracks: p.found.tracks,
          statsText: p.stats.text,
          video: p.video,
          stats: p.stats.name,
          run: null,
        });
        return { ...report, review_model: p.found.model };
      },
    });
  }

  tracks(id: () => string | undefined): ResourceRef<Tracks | null | undefined> {
    return resource({
      params: id,
      loader: async ({ params }) => this.found().get(params)?.tracks ?? null,
    });
  }

  /** Finds the recording's tracks in a worker; job() follows it. The scenario's file gives its target count. */
  async start(id: string): Promise<Job> {
    const why = this.unavailable(id);
    if (why) return { stage: 'error', error: why };
    const local = this.local.find(id);
    if (!local) return { stage: 'error', error: NOT_OPEN };
    const cap = this.scenarios.get(localRecording(local).scenario)?.targets ?? null;
    const list = this.models.list.hasValue() ? this.models.list.value() : undefined;
    const model = list?.chosen ?? DEFAULT_MODEL;
    const run: BrowserRun = { job: { stage: 'starting' } };
    this.runs.set(id, run);
    const worker = new Worker(new URL('./review.worker', import.meta.url), { type: 'module' });
    const base = new URL(document.baseURI);
    const request: ReviewRequest = {
      file: local.file,
      coreUrl: new URL('core/aimview.wasm', base).href,
      ortPath: new URL('ort/', base).href,
      modelUrl: new URL(`models/detector_${model}_u8in.onnx`, base).href,
      cap,
    };
    worker.onmessage = (e: MessageEvent<ReviewMessage>) => {
      const m = e.data;
      if (m.kind === 'progress') run.job = { stage: m.stage, done: m.done, total: m.total };
      else {
        if (m.kind === 'done')
          this.found.update((all) => new Map(all).set(id, { tracks: m.tracks, model }));
        run.job =
          m.kind === 'done'
            ? { stage: 'done', seconds: Math.round(m.seconds * 10) / 10 }
            : { stage: 'error', error: m.error };
        worker.terminate();
      }
    };
    worker.onerror = (e) => {
      run.job = { stage: 'error', error: e.message };
      worker.terminate();
    };
    worker.postMessage(request);
    return run.job;
  }

  async job(id: string): Promise<Job> {
    return this.runs.get(id)?.job ?? { stage: 'none' };
  }
}
