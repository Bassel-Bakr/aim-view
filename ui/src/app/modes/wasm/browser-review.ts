import { inject, Injectable, resource, ResourceRef } from '@angular/core';
import { Job, Report, Tracks } from '../../api';
import { ModelCatalog } from '../../platform/model-catalog';
import { ReviewEngine } from '../../platform/review-engine';
import { LocalFiles } from '../web-files/local-files';
import { ReviewMessage, ReviewRequest } from './review-messages';

const NOT_YET =
  'The review in the browser finds the targets now; measuring them is not built yet, so there is no report.';
const DEFAULT_MODEL = 'full_v3';

/** A recording's run in the worker: where it stands, and its tracks once found. */
export interface BrowserRun {
  job: Job;
  tracks: Tracks | null;
}

/**
 * The review in the browser: the detector (onnxruntime-web) and the review core (Rust, as WebAssembly), run in a
 * worker (review.worker.ts). It finds the tracks; the measures and the report come next, so the run page still says
 * the review is not built.
 */
@Injectable({ providedIn: 'root' })
export class BrowserReview implements ReviewEngine {
  private readonly local = inject(LocalFiles);
  private readonly models = inject(ModelCatalog);
  private readonly runs = new Map<string, BrowserRun>();

  unavailable(): string | null {
    return NOT_YET;
  }

  report(id: () => string | undefined): ResourceRef<Report | null | undefined> {
    return resource({ params: id, loader: async () => null });
  }

  tracks(id: () => string | undefined): ResourceRef<Tracks | null | undefined> {
    return resource({
      params: id,
      loader: async ({ params }) => this.runs.get(params)?.tracks ?? null,
    });
  }

  /**
   * Finds the recording's tracks in a worker; job() follows it. The scenario's target count is not known in the
   * browser yet (it is read from the scenario's file), so no cap is set.
   */
  async start(id: string): Promise<Job> {
    const why = this.unavailable();
    if (why) return { stage: 'error', error: why };
    const cap = null;
    const file = this.local.find(id)?.file;
    if (!file) return { stage: 'error', error: 'The recording is not open in this browser' };
    const list = this.models.list.hasValue() ? this.models.list.value() : undefined;
    const model = list?.chosen ?? DEFAULT_MODEL;
    const run: BrowserRun = { job: { stage: 'starting' }, tracks: null };
    this.runs.set(id, run);
    const worker = new Worker(new URL('./review.worker', import.meta.url), { type: 'module' });
    const base = new URL(document.baseURI);
    const request: ReviewRequest = {
      file,
      coreUrl: new URL('core/aimview.wasm', base).href,
      ortPath: new URL('ort/', base).href,
      modelUrl: new URL(`models/detector_${model}_u8in.onnx`, base).href,
      cap,
    };
    worker.onmessage = (e: MessageEvent<ReviewMessage>) => {
      const m = e.data;
      if (m.kind === 'progress') run.job = { stage: m.stage, done: m.done, total: m.total };
      else {
        run.job =
          m.kind === 'done'
            ? { stage: 'done', seconds: Math.round(m.seconds * 10) / 10 }
            : { stage: 'error', error: m.error };
        if (m.kind === 'done') run.tracks = m.tracks;
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
