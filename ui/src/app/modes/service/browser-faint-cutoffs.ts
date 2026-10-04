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
/** A crop's color channels (RGB), and a box's values (its corners). */
const RGB_CHANNELS = 3;
const BOX_VALUES = 4;
/** A clicking run's labels leave out what lies this near the crosshair (degrees): the target being shot. */
const CLICKING_NEAR_DEG = 2;

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
function cutoffSpan(report: Report): CutoffSpan {
  if (report.mode === 'track')
    return { start: report.summary.start, end: report.summary.end, near: 0 };
  if (!report.flicks.length) return { start: null, end: null, near: CLICKING_NEAR_DEG };
  return {
    start: Math.min(...report.flicks.map((flick) => flick.start_frame)),
    end: Math.max(...report.flicks.map((flick) => flick.kill_frame)),
    near: CLICKING_NEAR_DEG,
  };
}

/** The crops' pixels, read from the recording in a worker (cutoff.worker.ts). */
function readPixels(work: CutoffWork): Promise<CropPixels[]> {
  return new Promise((resolve, reject) => {
    const worker = new Worker(new URL('../wasm/cutoff.worker', import.meta.url), {
      type: 'module',
    });
    worker.onmessage = (event: MessageEvent<CutoffReply>) => {
      worker.terminate();
      if (event.data.kind === 'error') reject(new Error(event.data.error));
      else resolve(event.data.crops);
    };
    worker.onerror = (event) => {
      worker.terminate();
      reject(new Error(event.message));
    };
    worker.postMessage(work);
  });
}

/** A label's crop as the .npz hand_crops.py writes: its pixels, the fixed map, no mask, the boxes it keeps. */
function cropFile(crop: CutoffCrop, pixels: CropPixels): Promise<CutoffCropFile> {
  const boxes = new Float32Array(crop.boxes.flat());
  return npzFile([
    { name: 'rgb', descr: '|u1', shape: [CROP, CROP, RGB_CHANNELS], data: pixels.rgb },
    { name: 'fixed', descr: '|u1', shape: [CROP, CROP], data: pixels.fixed },
    { name: 'tmask', descr: '|u1', shape: [CROP, CROP], data: new Uint8Array(CROP * CROP) },
    {
      name: 'boxes',
      descr: '<f4',
      shape: [crop.boxes.length, BOX_VALUES],
      data: new Uint8Array(boxes.buffer),
    },
    { name: 'hidden', descr: '|u1', shape: [], data: new Uint8Array(1) },
  ]).then((npz) => ({ file: crop.row.file, npz }));
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
    const files = await Promise.all(crops.map((crop, index) => cropFile(crop, pixels[index])));
    await this.store.add(
      crops.map((crop) => crop.row),
      files,
    );
  }
}
