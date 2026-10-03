import { computed, inject, Injectable, resource, ResourceRef, signal } from '@angular/core';
import { Job, Report, Tracks } from '../../api';
import { ModelCatalog } from '../../platform/model-catalog';
import { ReviewEngine } from '../../platform/review-engine';
import { LocalFile, LocalFiles, localRecording } from '../web-files/local-files';
import { SavedReview, SavedReviews } from '../web-files/saved-reviews';
import { ScenarioFacts } from '../web-files/scenario-facts';
import { StatsCsv } from '../web-files/stats-csv';
import { CoreModule } from './core-module';
import { BrowserDevice, ReviewMessage, ReviewRequest, RunPart } from './review-messages';

const NOT_OPEN = 'The recording is not open in this browser.';
const NO_SCENARIOS =
  "Without this scenario's file the review does not know its target count or kind (a tracking run is reviewed as a " +
  "clicking one). Give KovaaK's scenario folders with Stats folder at the top.";
const NO_STATS =
  "Pair this run with its stats file to review it in the browser: choose KovaaK's stats folder with Stats folder " +
  'at the top, or its .csv in the stats file panel. Runs without one (read from the HUD or the video alone) are ' +
  'not reviewed here yet.';
const DEFAULT_MODEL = 'full_v3';
const DEVICE_NAMES: Record<BrowserDevice, string> = { webgpu: 'WebGPU', wasm: 'WebAssembly' };
/**
 * The runs a recording is split into on the GPU, each reviewed in a worker of its own (split-runs.ts): one software
 * decoder is the review's limit there, and two decode at almost twice the speed. On the CPU the detector is the limit:
 * one run. A computer with fewer than 8 threads has one run too.
 */
const GPU_RUNS = 2;

/** A recording's run in the worker: where it stands. */
export interface BrowserRun {
  job: Job;
}

/**
 * The review a recording shows: the model that made it (null: none), and its tracks and readings when they are in
 * memory (else they are read from the saved reviews).
 */
export interface ShownReview {
  id: string;
  model: string | null;
  found: SavedReview | undefined;
}

/** What a report is worked out from: the review shown, the video's and scenario's names, and the stats file. */
export interface ReportParams extends ShownReview {
  video: string;
  scenario: string;
  stats: StatsCsv | null;
}

function sameShown(a: ShownReview | undefined, b: ShownReview | undefined): boolean {
  return a?.id === b?.id && a?.model === b?.model && a?.found === b?.found;
}

function sameParams(a: ReportParams | undefined, b: ReportParams | undefined): boolean {
  return sameShown(a, b) && a?.stats === b?.stats;
}

/** A review in memory: the recording's and the model's. */
const foundKey = (id: string, model: string) => `${id}\n${model}`;

/**
 * The review in the browser: the detector (onnxruntime-web) and the review core (Rust, as WebAssembly). A worker
 * (review.worker.ts) finds the tracks; the core on the page measures them against the stats file, so a stats file
 * paired later gives a report without finding the tracks again. The tracks are saved in this browser (SavedReviews),
 * one review per model: a recording shows the chosen model's review, else its latest, without being reviewed again
 * after a reload. Clicking and tracking runs with a stats file so far.
 */
@Injectable({ providedIn: 'root' })
export class BrowserReview implements ReviewEngine {
  private readonly local = inject(LocalFiles);
  private readonly scenarios = inject(ScenarioFacts);
  private readonly models = inject(ModelCatalog);
  private readonly core = inject(CoreModule);
  private readonly saved = inject(SavedReviews);
  private readonly runs = new Map<string, BrowserRun>();
  /** The reviews in memory, by foundKey: made here, or read from the saved reviews. */
  private readonly found = signal<ReadonlyMap<string, SavedReview>>(new Map());
  /** Saved reviews being read, by foundKey, so each is read once. */
  private readonly restoring = new Map<string, Promise<SavedReview | null>>();

  unavailable(id: string): string | null {
    const f = this.local.find(id);
    if (!f) return NOT_OPEN;
    return f.stats ? null : NO_STATS;
  }

  /** A scenario no scenario file gives is reviewed as Python reviews it: no target count, a clicking run. */
  caveat(id: string): string | null {
    const f = this.local.find(id);
    if (!f?.stats || this.scenarios.get(localRecording(f).scenario)) return null;
    const from = this.scenarios.sources();
    return from.has('scenarios') && from.has('workshop') ? null : NO_SCENARIOS;
  }

  report(id: () => string | undefined): ResourceRef<Report | null | undefined> {
    const params = computed<ReportParams | undefined>(
      () => {
        const at = id();
        const f = at === undefined ? null : this.local.find(at);
        if (!f) return undefined;
        return {
          ...this.shown(f),
          video: f.file.name,
          scenario: localRecording(f).scenario,
          stats: f.stats,
        };
      },
      { equal: sameParams },
    );
    return resource({
      params,
      loader: async ({ params: p }) => {
        if (!p.stats || !p.model) return null;
        const found = p.found ?? (await this.restore(p.id, p.model));
        if (!found) return null;
        const facts = this.scenarios.get(p.scenario);
        const report = await this.core.report({
          tracks: found.tracks,
          statsText: p.stats.text,
          video: p.video,
          stats: p.stats.name,
          run: null,
          tracking: facts?.kind === 'tracking',
          limit: facts?.limit ?? null,
          ...found.readings,
        });
        return { ...report, review_model: found.model };
      },
    });
  }

  tracks(id: () => string | undefined): ResourceRef<Tracks | null | undefined> {
    const params = computed<ShownReview | undefined>(
      () => {
        const at = id();
        const f = at === undefined ? null : this.local.find(at);
        return f ? this.shown(f) : undefined;
      },
      { equal: sameShown },
    );
    return resource({
      params,
      loader: async ({ params: p }) => {
        if (!p.model) return null;
        return (p.found ?? (await this.restore(p.id, p.model)))?.tracks ?? null;
      },
    });
  }

  /** The model new reviews use. */
  private chosenModel(): string {
    const list = this.models.list.hasValue() ? this.models.list.value() : undefined;
    return list?.chosen ?? DEFAULT_MODEL;
  }

  /** The review a recording shows: the chosen model's when there is one, else its latest saved one. */
  private shown(f: LocalFile): ShownReview {
    const chosen = this.chosenModel();
    const model = this.found().has(foundKey(f.id, chosen))
      ? chosen
      : this.saved.shownModel(f.file, chosen);
    return { id: f.id, model, found: model ? this.found().get(foundKey(f.id, model)) : undefined };
  }

  /** A saved review read into memory (once); null when there is none to read. */
  private restore(id: string, model: string): Promise<SavedReview | null> {
    const k = foundKey(id, model);
    let reading = this.restoring.get(k);
    if (!reading) {
      const f = this.local.find(id);
      reading = f ? this.saved.load(f.file, model) : Promise.resolve(null);
      this.restoring.set(k, reading);
      void reading.then((review) => {
        this.restoring.delete(k);
        if (review) this.found.update((all) => new Map(all).set(k, review));
      });
    }
    return reading;
  }

  /** Finds the recording's tracks in a worker; job() follows it. The scenario's file gives its target count. */
  async start(id: string): Promise<Job> {
    const why = this.unavailable(id);
    if (why) return { stage: 'error', error: why };
    const local = this.local.find(id);
    if (!local) return { stage: 'error', error: NOT_OPEN };
    const cap = this.scenarios.get(localRecording(local).scenario)?.targets ?? null;
    const list = this.models.list.hasValue() ? this.models.list.value() : undefined;
    const model = this.chosenModel();
    const device: BrowserDevice = list?.device === 'wasm' ? 'wasm' : 'webgpu';
    const run: BrowserRun = { job: { stage: 'starting' } };
    this.runs.set(id, run);
    const begun = performance.now();
    const runs = device === 'webgpu' && navigator.hardwareConcurrency >= 8 ? GPU_RUNS : 1;
    const workers: Worker[] = [];
    const parts: (RunPart | null | undefined)[] = Array.from({ length: runs }, () => undefined);
    const done = parts.map(() => 0);
    const looking = parts.map(() => true);
    let total = 0;
    let over = false;
    const end = (job: Job) => {
      if (over) return;
      over = true;
      run.job = job;
      for (const w of workers) w.terminate();
    };
    /** The runs' parts joined into the review, which is kept and shown. */
    const join = async () => {
      run.job = { stage: 'linking', done: total, total };
      const got = parts.filter((p): p is RunPart => !!p);
      const joined = await this.core.joinRuns(got, cap ?? 0);
      const first = got[0];
      const tracks: Tracks = {
        fps: first.fps,
        frames: joined.frames,
        fixed: first.fixed.reduce((a, v) => a + v, 0) / first.fixed.length,
        detector: `onnxruntime-web (${DEVICE_NAMES[first.device]})`,
      };
      const review: SavedReview = { tracks, readings: joined.readings, model };
      this.found.update((all) => new Map(all).set(foundKey(id, model), review));
      // kept for the next visit; a browser that cannot keep it still shows it now
      this.saved.save(local.file, review).catch((err: unknown) => console.warn(err));
      end({ stage: 'done', seconds: Math.round((performance.now() - begun) / 100) / 10 });
    };
    const base = new URL(document.baseURI);
    for (let i = 0; i < runs; i++) {
      // each run's review worker, and the camera worker beside it: the two talk over a port of their own
      const worker = new Worker(new URL('./review.worker', import.meta.url), { type: 'module' });
      const camera = new Worker(new URL('./camera.worker', import.meta.url), { type: 'module' });
      workers.push(worker, camera);
      const channel = new MessageChannel();
      camera.postMessage(channel.port2, [channel.port2]);
      const request: ReviewRequest = {
        file: local.file,
        run: i,
        runs,
        coreUrl: new URL('core/aimview.wasm', base).href,
        ortPath: new URL('ort/', base).href,
        modelUrl: new URL(`models/detector_${model}_u8in.onnx`, base).href,
        device,
        batch: list?.batch ?? 1,
        cap,
        camera: channel.port1,
      };
      worker.onmessage = (e: MessageEvent<ReviewMessage>) => {
        const m = e.data;
        if (over) return;
        if (m.kind === 'error') return end({ stage: 'error', error: m.error });
        if (m.kind === 'progress') {
          done[i] = m.done;
          looking[i] = m.stage === 'looking';
          total = m.total;
          const stage = looking.some((l) => l) ? 'looking' : 'tracking';
          run.job = { stage, done: done.reduce((a, d) => a + d, 0), total };
          return;
        }
        parts[i] = m.part;
        looking[i] = false;
        worker.terminate();
        camera.terminate();
        if (parts.every((p) => p !== undefined)) {
          join().catch((err: unknown) =>
            end({ stage: 'error', error: err instanceof Error ? err.message : String(err) }),
          );
        }
      };
      worker.onerror = camera.onerror = (e) => end({ stage: 'error', error: e.message });
      worker.postMessage(request, [channel.port1]);
    }
    return run.job;
  }

  async job(id: string): Promise<Job> {
    return this.runs.get(id)?.job ?? { stage: 'none' };
  }
}
