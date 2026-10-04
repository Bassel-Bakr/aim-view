import { computed, inject, Service, signal } from '@angular/core';
import { CutoffLabelsCount } from '../../platform/faint-cutoffs';
import { BrowserStore, StoreEntry } from './browser-store';
import { zipFile } from './zip-file';

const ROWS_KEY = 'cutoff-rows';
const CROP_KEY = 'cutoff-crop:';

/** The name the labels download as. */
export const LABELS_FILE = 'cutoff.zip';

/**
 * A label's row in checked.jsonl (the core's CutoffRow: python/model/hand_crops.py `cutoff_crops`): its crop's file,
 * the boxes it keeps and every box the model gave there (crop pixels: center, size), its verdict and source, the
 * recording, the cut-off's offset and the score it cut at.
 */
export interface CutoffRow {
  file: string;
  boxes: number[][];
  verdict: string;
  model: number[][];
  source: string;
  video: string;
  offset: number;
  cut: number;
}

/** A label's crop file (train/<name>.npz) and its bytes. */
export interface CutoffCropFile {
  file: string;
  npz: Uint8Array<ArrayBuffer>;
}

/** A number as Python's json.dumps writes a float: 128.0, 0.3 (the labels' numbers lie between 1e-4 and 1e16). */
function pythonFloat(value: number): string {
  return Number.isInteger(value) ? value.toFixed(1) : String(value);
}

/** A value as Python's json.dumps writes it, floats as floats. */
function pythonJson(value: unknown): string {
  if (typeof value === 'number') return pythonFloat(value);
  if (Array.isArray(value)) return `[${value.map(pythonJson).join(', ')}]`;
  if (value && typeof value === 'object')
    return `{${Object.entries(value)
      .map(([key, field]) => `${JSON.stringify(key)}: ${pythonJson(field)}`)
      .join(', ')}}`;
  // past ASCII as \u escapes, as json.dumps writes it (ensure_ascii)
  return JSON.stringify(value).replace(
    /[\u007f-￿]/g,
    (char) => `\\u${char.charCodeAt(0).toString(16).padStart(4, '0')}`,
  );
}

/** A row as hand_crops.py writes it in checked.jsonl. */
export function rowLine(row: CutoffRow): string {
  return pythonJson(row);
}

/**
 * The detector labels the cut-offs submitted in this browser wrote, kept in it (IndexedDB): the rows of checked.jsonl,
 * in the order written (a later submit's rows win, as training reads them), and each crop's .npz. They download as one
 * zip holding checked.jsonl and train/, as the review server's test_out/vod_model/hand/cutoff/ holds them.
 */
@Service()
export class CutoffLabels {
  private readonly store = inject(BrowserStore);
  private readonly rows = signal<readonly CutoffRow[]>([]);
  readonly ready: Promise<void>;
  readonly count = computed<CutoffLabelsCount>(() => {
    const rows = this.rows();
    return {
      crops: new Set(rows.map((row) => row.file)).size,
      recordings: new Set(rows.map((row) => row.video)).size,
    };
  });

  constructor() {
    this.ready = this.store.get<CutoffRow[]>(ROWS_KEY).then((kept) => {
      if (kept) this.rows.update((now) => [...kept, ...now]);
    });
  }

  /** Keeps a submit's labels: its rows after the ones before, and its crops in place of any of the same name. */
  async add(rows: readonly CutoffRow[], crops: readonly CutoffCropFile[]): Promise<void> {
    await this.ready;
    this.rows.update((now) => [...now, ...rows]);
    const entries: StoreEntry[] = crops.map((crop) => [CROP_KEY + crop.file, crop.npz]);
    await this.store.setMany([...entries, [ROWS_KEY, this.rows()]]);
  }

  /** The labels as the zip training reads: checked.jsonl and train/*.npz. */
  async file(): Promise<Blob> {
    await this.ready;
    const rows = this.rows();
    const names = [...new Set(rows.map((row) => row.file))];
    const crops = await Promise.all(
      names.map(async (file) => ({
        file,
        npz: await this.store.get<Uint8Array<ArrayBuffer>>(CROP_KEY + file),
      })),
    );
    const zip = await zipFile([
      {
        path: 'checked.jsonl',
        data: new TextEncoder().encode(rows.map((row) => `${rowLine(row)}\n`).join('')),
        deflate: true,
      },
      ...crops
        .filter((crop): crop is CutoffCropFile => !!crop.npz)
        .map((crop) => ({ path: crop.file, data: crop.npz, deflate: false })),
    ]);
    return new Blob([zip], { type: 'application/zip' });
  }
}
