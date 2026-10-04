import { HttpClient } from '@angular/common/http';
import { inject, Service } from '@angular/core';
import { firstValueFrom } from 'rxjs';
import { AreaRect, FaintSetting, RecordingAreas, Report, Tracks } from '../../api';
import { CutoffLabelsStore } from '../../platform/faint-cutoffs';
import { ServerFaintCutoffs } from '../http/server-faint-cutoffs';
import { CoreModule, CutoffCrop } from '../wasm/core-module';
import { CropPixels, CutoffReply, CutoffWork } from '../wasm/cutoff-messages';
import { CutoffCropFile, CutoffLabels, LABELS_FILE } from '../web-files/cutoff-labels';
import { npzFile } from '../web-files/npz-file';
import { MountedFiles, recordingPath } from './mounted-files';

/** A label's crop: 256 pixels square. */
const CROP = 256;

/** What a crop's label is worked out from: the run's first and last frames, and the nearness that does not count. */
interface CutoffSpan {
  start: number | null;
  end: number | null;
  near: number;
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
    const worker = new Worker(new URL('../wasm/cutoff.worker', import.meta.url), {
      type: 'module',
    });
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
 * Browser mode's cut-offs: the review service keeps each recording's, as the review server does. A submit's detector
 * labels are made by the page, in the background (the service has no ffmpeg here): the core picks the crops as
 * hand_crops.py does, a worker reads their pixels, and they are kept in this browser (CutoffLabels), which the user
 * downloads as cutoff.zip.
 */
@Service()
export class BrowserFaintCutoffs extends ServerFaintCutoffs {
  private readonly client = inject(HttpClient);
  private readonly files = inject(MountedFiles);
  private readonly core = inject(CoreModule);
  private readonly store = inject(CutoffLabels);

  override readonly labels: CutoffLabelsStore = {
    count: this.store.count,
    fileName: LABELS_FILE,
    file: () => this.store.file(),
  };

  override async submit(id: string, offset: number): Promise<FaintSetting> {
    const kept = await super.submit(id, offset);
    this.writeLabels(id, offset).catch((err: unknown) => console.warn(err));
    return kept;
  }

  /** The submit's labels, from the recording's review and areas as the service has them. */
  private async writeLabels(id: string, offset: number): Promise<void> {
    const params = { id };
    const [report, tracks, areas] = await Promise.all([
      firstValueFrom(this.client.get<Report | null>('/api/report', { params })),
      firstValueFrom(this.client.get<Tracks | null>('/api/tracks', { params })),
      firstValueFrom(this.client.get<RecordingAreas>('/api/exclude', { params })),
    ]);
    if (!report || !tracks) return;
    const span = cutoffSpan(report);
    if (span.start === null) return;
    const exclude = areas.boxes.map(([x0, y0, x1, y1]): AreaRect => [x0, y0, x1, y1]);
    const crops = await this.core.cutoffCrops({
      frames: tracks.frames,
      video: id.split('/').pop() ?? id,
      start: span.start,
      end: span.end,
      exclude,
      offset,
      near: span.near,
    });
    if (!crops.length) return;
    const pixels = await readPixels({
      file: await this.files.read(recordingPath(id)),
      coreUrl: new URL('core/aimview.wasm', document.baseURI).href,
      crops: crops.map(({ frame, x0, y0 }) => ({ frame, x0, y0 })),
    });
    const files = await Promise.all(crops.map((c, k) => cropFile(c, pixels[k])));
    await this.store.add(
      crops.map((c) => c.row),
      files,
    );
  }
}
