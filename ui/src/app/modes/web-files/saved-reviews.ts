import { inject, Injectable, signal } from '@angular/core';
import { Tracks } from '../../api';
import { FinderResult } from '../wasm/area-finder-messages';
import { HudReading, VideoReadings } from '../wasm/review-messages';
import { BrowserStore } from './browser-store';

const INDEX = 'review-index';
const KEY = 'review:';

/**
 * A recording's tracks, video readings and what its HUD read (null: nothing), as the browser review found them, and the
 * model that found them. A review saved before the HUD was read has no `hud`: it reads as null. A review saved while
 * the area finder ran in the review's first run also has what the finder found (`found`), which BrowserAreaFinder
 * reads when it has kept nothing of its own for the recording.
 */
export interface SavedReview {
  tracks: Tracks;
  readings: VideoReadings;
  hud?: HudReading | null;
  model: string;
  found?: FinderResult | null;
}

/** When each model's review of a recording was saved (milliseconds since 1970), by model. */
export type SavedModels = Record<string, number>;

/** Every saved review, by recording (its file's fingerprint). */
type SavedIndex = Record<string, SavedModels>;

/** A file as its saved reviews know it: its name, size and date, so a changed file is not matched to an old review. */
export function fingerprint(file: File): string {
  return `${file.name}|${file.size}|${file.lastModified}`;
}

const key = (file: File, model: string) => `${KEY}${fingerprint(file)}|${model}`;

/**
 * The browser review's results, kept in this browser (IndexedDB) so a run is not reviewed again after a reload: each
 * recording's tracks, video readings and HUD reading, one review per model, as the review server keeps them. The report is not
 * kept: it is worked out again from them and the stats file, so it follows the review code. Clearing the list keeps
 * them, so the same file added again finds its reviews.
 */
@Injectable({ providedIn: 'root' })
export class SavedReviews {
  private readonly store = inject(BrowserStore);
  /** Which recordings have saved reviews, and by which models. */
  readonly index = signal<ReadonlyMap<string, SavedModels>>(new Map());
  /** The index as kept in the store, read once. */
  private readonly read: Promise<void>;

  constructor() {
    this.read = this.store.get<SavedIndex>(INDEX).then((kept) => {
      if (kept) this.index.update((now) => new Map([...Object.entries(kept), ...now]));
    });
  }

  /** Whether the file has a saved review. */
  has(file: File): boolean {
    return this.index().has(fingerprint(file));
  }

  /** The model whose review to show: the chosen one's when it is saved, else the latest saved; null for none. */
  shownModel(file: File, chosen: string): string | null {
    const models = this.index().get(fingerprint(file));
    if (!models) return null;
    if (chosen in models) return chosen;
    return Object.entries(models).sort((a, b) => b[1] - a[1])[0]?.[0] ?? null;
  }

  /** A saved review; null when the store has none (or there is no store). */
  async load(file: File, model: string): Promise<SavedReview | null> {
    await this.read;
    return (await this.store.get<SavedReview>(key(file, model))) ?? null;
  }

  /** What the area finder found, as a review saved while the finder ran in the review kept it; null when none did. */
  async finderResult(file: File): Promise<FinderResult | null> {
    await this.read;
    for (const model of Object.keys(this.index().get(fingerprint(file)) ?? {})) {
      const found = (await this.load(file, model))?.found;
      if (found) return found;
    }
    return null;
  }

  /** Keeps a review, in place of the same model's earlier one. The index says so at once. */
  async save(file: File, review: SavedReview): Promise<void> {
    await this.read;
    const fp = fingerprint(file);
    this.index.update((all) =>
      new Map(all).set(fp, { ...all.get(fp), [review.model]: Date.now() }),
    );
    await this.store.setMany([
      [key(file, review.model), review],
      [INDEX, Object.fromEntries(this.index())],
    ]);
  }
}
