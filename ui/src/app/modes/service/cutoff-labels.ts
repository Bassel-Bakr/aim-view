/**
 * The detector labels browser mode's cut-off submits make, kept by the review service in its
 * database (/api/cutoff_labels), as the native submit keeps them. In: each submit's rows and crop
 * files (browser-faint-cutoffs.ts). Out: their count, and the zip of checked.jsonl and train/*.npz
 * the user downloads for detector training, as python/model/hand_crops.py writes them. Before
 * 2026-10-07 the page kept them in IndexedDB itself (ui/retired/modes/web-files/cutoff-labels.ts);
 * service/cutoff-labels-move.ts moves those once.
 */

import { HttpClient, httpResource } from '@angular/common/http';
import { computed, inject, Service } from '@angular/core';
import { firstValueFrom } from 'rxjs';
import { CutoffLabelsCount } from '../../platform/faint-cutoffs';
import { encodeBatch } from './kovaak-batch';

/** The name the labels download as. */
export const LABELS_FILE = 'cutoff.zip';

/** The service's route for the labels: their zip, their count (?count=1), and a submit's (POST). */
const LABELS_ROUTE = '/api/cutoff_labels';

/** A submit's rows in its batch, as the service reads them. */
export const ROWS_FILE = 'rows.json';

/** No labels, until the service says how many it keeps. */
const NONE: CutoffLabelsCount = { crops: 0, recordings: 0 };

/**
 * A label's row in checked.jsonl (the core's CutoffRow: python/model/hand_crops.py
 * `cutoff_crops`): its crop's file, the boxes it keeps and every box the model gave there (crop
 * pixels: center, size), its verdict and source, the recording, the cut-off's offset and the score
 * it cut at.
 */
export interface CutoffRow {
  /** The crop's file in the zip: train/<stem>_<frame, 6 digits>.npz. */
  file: string;
  /** The boxes of the tracks the cut keeps, each [center x, center y, width, height] in crop px. */
  boxes: number[][];
  /** Always "correct": training takes the label as checked. */
  verdict: string;
  /** Every box the model gave in the crop, as boxes holds them. */
  model: number[][];
  /** Always "cutoff": the label came from a cut-off. */
  source: string;
  /** The recording the crop was read from, as the review request named it. */
  video: string;
  /** The cut-off's offset, as the user set it. */
  offset: number;
  /** The score the cut was at, rounded to 3 decimals: tracks below it are left out. */
  cut: number;
}

/** A label's crop file (train/<name>.npz) and its bytes. */
export interface CutoffCropFile {
  /** Its path in the zip, as the rows' file names it. */
  file: string;
  /** The .npz file's bytes (npz-file.ts). */
  npz: Uint8Array<ArrayBuffer>;
}

/**
 * A submit's labels as the service takes them (POST /api/cutoff_labels, kovaak-batch.ts's form):
 * each crop at its path, then the rows.
 */
export function labelsBatch(
  rows: readonly CutoffRow[],
  crops: readonly CutoffCropFile[],
): Promise<Uint8Array> {
  const files = crops.map(({ file, npz }) => ({ path: file, file: new File([npz], file) }));
  const rowsFile = new File([JSON.stringify(rows)], ROWS_FILE);
  return encodeBatch([...files, { path: ROWS_FILE, file: rowsFile }]);
}

/**
 * The detector labels the cut-offs submitted in this browser made, kept by the review service:
 * a later submit's rows win, as training reads them. They download as one zip holding
 * checked.jsonl and train/, as the review server's test_out/vod_model/hand/cutoff/ holds them.
 */
@Service()
export class CutoffLabels {
  /** Sends a submit's labels and reads the zip. */
  private readonly http = inject(HttpClient);
  /** How many labels the service keeps, read again after each submit. */
  private readonly counted = httpResource<CutoffLabelsCount>(() => ({
    url: LABELS_ROUTE,
    params: { count: '1' },
  }));
  /** How many crops (by file) and recordings the labels cover, for the cut-off's download button. */
  readonly count = computed<CutoffLabelsCount>(() =>
    this.counted.hasValue() ? this.counted.value() : NONE,
  );

  /** Keeps a submit's labels in the service: its crops in place of any of the same name, its rows. */
  async add(rows: readonly CutoffRow[], crops: readonly CutoffCropFile[]): Promise<void> {
    await firstValueFrom(this.http.post(LABELS_ROUTE, await labelsBatch(rows, crops)));
    this.counted.reload();
  }

  /** The labels as the zip training reads (the service builds it). */
  file(): Promise<Blob> {
    return firstValueFrom(this.http.get(LABELS_ROUTE, { responseType: 'blob' }));
  }
}
