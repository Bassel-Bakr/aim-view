import { inject, Injectable, resource, ResourceRef } from '@angular/core';
import { AreaRect, FaintChoice, FaintSetting, Job, Report } from '../../api';
import { CutoffLabelsStore, FaintCutoffs } from '../../platform/faint-cutoffs';
import { exampleRec } from '../web-files/area-examples';
import { labelQueue } from '../web-files/browser-labelling';
import { CutoffCropFile, CutoffLabels, LABELS_FILE } from '../web-files/cutoff-labels';
import { LocalFiles } from '../web-files/local-files';
import { npzFile } from '../web-files/npz-file';
import { DEFAULT_OFFSET, SavedFaint } from '../web-files/saved-faint';
import { fingerprint } from '../web-files/saved-reviews';
import { BrowserAreaLabels } from './browser-area-labels';
import { BrowserReview, ReviewShown } from './browser-review';
import { CoreModule, CutoffCrop } from './core-module';
import { CropPixels, CutoffReply, CutoffWork } from './cutoff-messages';

const NOT_OPEN = 'The recording is not open in this browser.';
const NO_REVIEW = 'review the recording first';
/** The offsets the cut-off takes (python/server.py: `set_faint`). */
const LOWEST = 0.2;
const HIGHEST = 0.6;
/** A label's crop: 256 pixels square. */
const CROP = 256;

/** What a crop's label is worked out from: the run's first and last frames, and the nearness that does not count. */
interface CutoffSpan {
  start: number | null;
  end: number | null;
  near: number;
}

/** The time as Python's datetime.now().isoformat(timespec="seconds") writes it: local, to the second. */
function localStamp(d: Date): string {
  const two = (n: number) => String(n).padStart(2, '0');
  return (
    `${d.getFullYear()}-${two(d.getMonth() + 1)}-${two(d.getDate())}T` +
    `${two(d.getHours())}:${two(d.getMinutes())}:${two(d.getSeconds())}`
  );
}

/**
 * The run's frames a submit's labels come from (python/server.py: `submit_faint`): a tracking run's own, with the
 * scores near the crosshair counted (its bot is under it); a clicking run's first flick's start to its last kill.
 */
function cutoffSpan(r: Report): CutoffSpan {
  if (r.mode === 'track') return { start: r.summary.start, end: r.summary.end, near: 0 };
  if (!r.flicks.length) return { start: null, end: null, near: 2 };
  return {
    start: Math.min(...r.flicks.map((m) => m.start_frame)),
    end: Math.max(...r.flicks.map((m) => m.kill_frame)),
    near: 2,
  };
}

/** The crops' pixels, read from the recording in a worker (cutoff.worker.ts). */
function readPixels(work: CutoffWork): Promise<CropPixels[]> {
  return new Promise((resolve, reject) => {
    const worker = new Worker(new URL('./cutoff.worker', import.meta.url), { type: 'module' });
    worker.onmessage = (e: MessageEvent<CutoffReply>) => {
      worker.terminate();
      if (e.data.kind === 'error') reject(new Error(e.data.error));
      else resolve(e.data.crops);
    };
    worker.onerror = (e) => {
      worker.terminate();
      reject(new Error(e.message));
    };
    worker.postMessage(work);
  });
}

/** A label's crop as the .npz hand_crops.py writes: its pixels, the fixed map, no mask, the boxes it keeps. */
function cropFile(c: CutoffCrop, px: CropPixels): Promise<CutoffCropFile> {
  const boxes = new Float32Array(c.boxes.flat());
  return npzFile([
    { name: 'rgb', descr: '|u1', shape: [CROP, CROP, 3], data: px.rgb },
    { name: 'fixed', descr: '|u1', shape: [CROP, CROP], data: px.fixed },
    { name: 'tmask', descr: '|u1', shape: [CROP, CROP], data: new Uint8Array(CROP * CROP) },
    { name: 'boxes', descr: '<f4', shape: [c.boxes.length, 4], data: new Uint8Array(boxes.buffer) },
    { name: 'hidden', descr: '|u1', shape: [], data: new Uint8Array(1) },
  ]).then((npz) => ({ file: c.row.file, npz }));
}

/**
 * The cut-off in the browser: each recording's kept in it (SavedFaint), and a tracking run's report worked out with
 * it by the core (BrowserReview). A submit writes its labels here (CutoffLabels): the core picks the crops as
 * hand_crops.py does, a worker reads their pixels, and the user downloads them as the files training reads.
 */
@Injectable({ providedIn: 'root' })
export class BrowserFaintCutoffs implements FaintCutoffs {
  private readonly local = inject(LocalFiles);
  private readonly saved = inject(SavedFaint);
  private readonly review = inject(BrowserReview);
  private readonly areas = inject(BrowserAreaLabels);
  private readonly core = inject(CoreModule);
  private readonly store = inject(CutoffLabels);

  readonly labels: CutoffLabelsStore = {
    count: this.store.count,
    fileName: LABELS_FILE,
    file: () => this.store.file(),
  };

  setting(id: () => string | undefined): ResourceRef<FaintSetting | undefined> {
    return resource({
      params: () => {
        const at = id();
        const f = at === undefined ? null : this.local.find(at);
        return f ? { file: f.file, kept: this.saved.all().get(fingerprint(f.file)) } : undefined;
      },
      loader: async ({ params: p }) => {
        await this.saved.ready;
        return p.kept ?? this.saved.get(p.file);
      },
    });
  }

  /** Kept here; the report follows at once (BrowserReview works it out with the cut-off): no job to follow. */
  async save(id: string, choice: FaintChoice): Promise<Job> {
    await this.keep(id, choice);
    return { stage: 'none' };
  }

  /** Kept as python/server.py's set_faint keeps it: a later change keeps the record of the last submit. */
  private async keep(id: string, choice: FaintChoice, submitted?: string): Promise<FaintSetting> {
    const f = this.local.find(id);
    if (!f) throw new Error(NOT_OPEN);
    const offset = Number(choice.offset ?? DEFAULT_OFFSET);
    if (!(offset >= LOWEST && offset <= HIGHEST)) {
      throw new Error(`offset must be between ${LOWEST} and ${HIGHEST}`);
    }
    await this.saved.ready;
    const old = this.saved.get(f.file);
    const next: FaintSetting = { on: !!choice.on, offset: Math.round(offset * 100) / 100 };
    if (submitted) Object.assign(next, { submitted, labels: null });
    else if (old.submitted)
      Object.assign(next, { submitted: old.submitted, labels: old.labels ?? null });
    await this.saved.save(f.file, next);
    return next;
  }

  async submit(id: string, offset: number): Promise<FaintSetting> {
    const shown = await this.review.shownReview(id);
    if (!shown) throw new Error(NO_REVIEW);
    const kept = await this.keep(id, { on: true, offset }, localStamp(new Date()));
    // the labels are written in the background, as the review server writes them
    this.writeLabels(id, shown, offset).catch((err: unknown) => console.warn(err));
    return kept;
  }

  /** The submit's labels: the crops the core picks, their pixels read from the recording, kept as .npz files. */
  private async writeLabels(id: string, shown: ReviewShown, offset: number): Promise<void> {
    const local = this.local.find(id);
    if (!local) return;
    const span = cutoffSpan(shown.report);
    const exclude = (await this.areas.tracked(local)).map(([x0, y0, x1, y1]): AreaRect => [
      x0,
      y0,
      x1,
      y1,
    ]);
    const crops =
      span.start === null
        ? []
        : await this.core.cutoffCrops({
            frames: shown.review.tracks.frames,
            video: shown.file.name,
            start: span.start,
            end: span.end,
            exclude,
            offset,
            near: span.near,
          });
    if (crops.length) {
      const pixels = await readPixels({
        file: shown.file,
        coreUrl: new URL('core/aimview.wasm', document.baseURI).href,
        crops: crops.map(({ frame, x0, y0 }) => ({ frame, x0, y0 })),
      });
      const files = await Promise.all(crops.map((c, k) => cropFile(c, pixels[k])));
      await this.store.add(
        crops.map((c) => c.row),
        files,
      );
    }
    const now = this.saved.get(shown.file);
    await this.saved.save(shown.file, { ...now, labels: crops.length });
  }

  async queue(): Promise<string[]> {
    await this.saved.ready;
    const skipped = new Set<string>();
    const submitted = new Set<string>();
    for (const f of this.local.files()) {
      const fp = fingerprint(f.file);
      if (this.saved.skipped().has(fp)) skipped.add(exampleRec(f.id));
      if (this.saved.all().get(fp)?.submitted) submitted.add(exampleRec(f.id));
    }
    return labelQueue(this.local.recordings(), skipped, submitted);
  }

  async skip(id: string): Promise<void> {
    const f = this.local.find(id);
    if (!f) throw new Error(NOT_OPEN);
    await this.saved.skip(f.file);
  }
}
