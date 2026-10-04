import { HttpClient } from '@angular/common/http';
import { inject, Service } from '@angular/core';
import { firstValueFrom } from 'rxjs';
import { AreaBox, errorMessage, Job, JobStage, RunMarks, TimeWindow, Tracks } from '../../api';
import { ServerReview } from '../http/server-review';
import { CoreModule } from '../wasm/core-module';
import {
  BrowserDevice,
  HudReading,
  ReviewMessage,
  ReviewRequest,
  RunPart,
  VideoReadings,
} from '../wasm/review-messages';
import { MountedFiles } from './mounted-files';
import { PageAreaFinder } from './page-area-finder';

/** The part of the video to track, as the service gives it: an open end is null (JSON has no Infinity). */
interface OrderWindow {
  start: number;
  end: number | null;
}

/**
 * A review the service leaves to the page (the contract's `review`): the video (a mounted path), the model, where to
 * run the detector and how many frames it takes at once, the scenario's target count (0 or null: not known), the part
 * of the video to track (null: all of it) and the areas the review leaves out.
 */
export interface ReviewOrder {
  video: string;
  model: string;
  device: BrowserDevice;
  batch: number;
  cap: number | null;
  window: OrderWindow | null;
  areas: AreaBox[];
}

/** A job the service answers: with `review` when it waits for the page to run that review. */
export interface OrderedJob extends Job {
  review?: ReviewOrder;
}

/** What the page sends once its review is done (POST /api/reviewed): the joined review, its time and its device. */
interface ReviewedBody {
  model: string;
  tracks: Tracks;
  readings: VideoReadings;
  hud: HudReading | null;
  found: null;
  seconds: number;
  device: string;
}

/** The page's progress on a review it runs (POST /api/job), or why it failed. */
interface JobReport {
  stage?: JobStage;
  done?: number;
  total?: number;
  error?: string;
}

const DEVICE_NAMES: Record<BrowserDevice, string> = { webgpu: 'WebGPU', wasm: 'WebAssembly' };
/**
 * The runs a recording is split into on the GPU, each reviewed in a worker of its own (src/session.rs): one software
 * decoder is the review's limit there, and two decode at almost twice the speed. On the CPU the detector is the limit:
 * one run. A computer with fewer than 8 threads has one run too.
 */
const GPU_RUNS = 2;
/** The fewest threads a computer needs for the GPU's runs. */
const GPU_RUNS_MIN_THREADS = 8;
/** How often the page tells the service how far its review is, at most (milliseconds). */
const REPORT_MS = 250;

/** A job as the page shows it: without the review it asks for. */
function plainJob(job: OrderedJob): Job {
  const out: OrderedJob = { ...job };
  delete out.review;
  return out;
}

/** The window for the review worker: an open end as Infinity. */
function workerWindow(orderWindow: OrderWindow | null): TimeWindow | null {
  return orderWindow ? { start: orderWindow.start, end: orderWindow.end ?? Infinity } : null;
}

/** What one run's review worker is asked: the video, its run of `runs`, the model and where the files are. */
function reviewRequest(
  file: Blob,
  order: ReviewOrder,
  run: number,
  runs: number,
  camera: MessagePort,
): ReviewRequest {
  const base = new URL(document.baseURI);
  return {
    file,
    run,
    runs,
    window: workerWindow(order.window),
    coreUrl: new URL('core/aimview.wasm', base).href,
    ortPath: new URL('ort/', base).href,
    modelUrl: new URL(`models/detector_${order.model}_u8in.onnx`, base).href,
    device: order.device,
    batch: order.batch || 1,
    cap: order.cap || null,
    areas: order.areas,
    camera,
  };
}

/** A time in seconds, rounded to tenths, from milliseconds. */
function tenthsOfSecond(ms: number): number {
  return Math.round(ms / 100) / 10;
}

/**
 * Browser mode's reviews: the review service in the page decides what to review and keeps the reviews, as the review
 * server does, and the page runs the review itself when the service asks it to (a job with `review`): the review and
 * camera workers on the video (the detector with onnxruntime-web, the core as WebAssembly), its progress sent to the
 * service, the runs joined by the core (core-module.ts), and the review sent to the service (POST /api/reviewed).
 * Then the area finder reads the recording, once, when the service has nothing found for it.
 */
@Service()
export class BrowserReview extends ServerReview {
  private readonly client = inject(HttpClient);
  private readonly files = inject(MountedFiles);
  private readonly core = inject(CoreModule);
  private readonly finder = inject(PageAreaFinder);
  /** The reviews the page runs, by recording: where each stands. */
  private readonly running = new Map<string, Job>();

  override async start(id: string, again: boolean): Promise<Job> {
    return this.follow(id, await super.start(id, again));
  }

  /** The page's own review while it runs; else the service's job, which may ask the page to run one. */
  override async job(id: string): Promise<Job> {
    const mine = this.running.get(id);
    if (mine) return mine;
    return this.follow(id, await super.job(id));
  }

  override async setMarks(id: string, marks: RunMarks | null): Promise<Job> {
    return this.follow(id, await super.setMarks(id, marks));
  }

  /** Runs the review a job asks for (once), and answers the job as the page runs it. */
  follow(id: string, job: OrderedJob): Job {
    const order = job.review;
    const mine = this.running.get(id);
    if (mine) return mine;
    if (!order) return job;
    const shown = plainJob(job);
    this.running.set(id, shown);
    void this.run(id, order);
    return shown;
  }

  /** The review the service asked for: run in workers, reported as it goes, sent to the service when done. */
  private async run(id: string, order: ReviewOrder): Promise<void> {
    const begun = performance.now();
    let sent = 0;
    let sentStage: JobStage | undefined;
    const report = (job: Job) => {
      this.running.set(id, job);
      const now = performance.now();
      if (job.stage === sentStage && now - sent < REPORT_MS) return;
      sent = now;
      sentStage = job.stage;
      const body: JobReport = { stage: job.stage, done: job.done, total: job.total };
      this.tell(id, body).catch(() => undefined);
    };
    try {
      const file = await this.files.read(order.video);
      const parts = await this.track(file, order, report);
      report({ stage: 'linking', done: 0, total: 0 });
      const detector = `onnxruntime-web (${DEVICE_NAMES[parts[0].device]})`;
      const joined = await this.core.joinReview(parts, detector);
      const body: ReviewedBody = {
        model: order.model,
        // the window as the service gave it
        tracks: { ...joined.tracks, window: order.window as TimeWindow | null },
        readings: joined.readings,
        hud: joined.hud,
        found: null,
        seconds: tenthsOfSecond(performance.now() - begun),
        device: DEVICE_NAMES[order.device],
      };
      await firstValueFrom(this.client.post<Job>('/api/reviewed', body, { params: { id } }));
      this.running.delete(id);
      // then the area finder, once a recording, when the service has nothing found for it. Run beside the review, it
      // slowed the review down (a recording with 11 key frames: 21 to 40 s, against 21 to 28 s without it).
      this.finder.ensure(id).catch((error: unknown) => console.warn(error));
    } catch (caught) {
      const error = errorMessage(caught);
      this.running.set(id, { stage: 'error', error });
      await this.tell(id, { error }).catch(() => undefined);
      this.running.delete(id);
    }
  }

  /** Sends the review's progress (or failure) to the service. */
  private async tell(id: string, body: JobReport): Promise<void> {
    await firstValueFrom(this.client.post('/api/job', body, { params: { id } }));
  }

  /**
   * The recording's runs tracked, each in a review worker with a camera worker beside it (the two talk over a port of
   * their own); `report` hears how far they are.
   */
  private track(file: Blob, order: ReviewOrder, report: (job: Job) => void): Promise<RunPart[]> {
    const split =
      order.device === 'webgpu' && navigator.hardwareConcurrency >= GPU_RUNS_MIN_THREADS;
    const runs = split ? GPU_RUNS : 1;
    const workers: Worker[] = [];
    const parts: (RunPart | null | undefined)[] = Array.from({ length: runs }, () => undefined);
    const done = parts.map(() => 0);
    const looking = parts.map(() => true);
    return new Promise<RunPart[]>((resolve, reject) => {
      let over = false;
      const end = (error: string | null) => {
        if (over) return;
        over = true;
        for (const worker of workers) worker.terminate();
        if (error !== null) reject(new Error(error));
        else resolve(parts.filter((part): part is RunPart => !!part));
      };
      for (let i = 0; i < runs; i++) {
        const worker = new Worker(new URL('../wasm/review.worker', import.meta.url), {
          type: 'module',
        });
        const camera = new Worker(new URL('../wasm/camera.worker', import.meta.url), {
          type: 'module',
        });
        workers.push(worker, camera);
        const channel = new MessageChannel();
        camera.postMessage(channel.port2, [channel.port2]);
        const request = reviewRequest(file, order, i, runs, channel.port1);
        worker.onmessage = (event: MessageEvent<ReviewMessage>) => {
          const message = event.data;
          if (over) return;
          if (message.kind === 'error') return end(message.error);
          if (message.kind === 'progress') {
            done[i] = message.done;
            looking[i] = message.stage === 'looking';
            const stage = looking.some((isLooking) => isLooking) ? 'looking' : 'tracking';
            report({
              stage,
              done: done.reduce((a, partDone) => a + partDone, 0),
              total: message.total,
            });
            return;
          }
          parts[i] = message.part;
          looking[i] = false;
          worker.terminate();
          camera.terminate();
          if (parts.every((part) => part !== undefined)) end(null);
        };
        worker.onerror = camera.onerror = (event) => end(event.message);
        worker.postMessage(request, [channel.port1]);
      }
    });
  }
}
