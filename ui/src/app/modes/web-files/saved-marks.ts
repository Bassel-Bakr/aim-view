import { inject, Injectable, signal } from '@angular/core';
import { RunMarks } from '../../api';
import { BrowserStore } from './browser-store';
import { fingerprint } from './saved-reviews';

const KEY = 'run-marks';

/** Every recording's run window, by its file's fingerprint. */
type MarksIndex = Record<string, RunMarks>;

/**
 * The user's run window for each recording opened in this browser, kept in it (IndexedDB) as the review server keeps
 * run.json: the same file added again finds its window.
 */
@Injectable({ providedIn: 'root' })
export class SavedMarks {
  private readonly store = inject(BrowserStore);
  /** The windows by fingerprint; read from the store once. */
  readonly all = signal<ReadonlyMap<string, RunMarks>>(new Map());
  private readonly read: Promise<void>;

  constructor() {
    this.read = this.store.get<MarksIndex>(KEY).then((kept) => {
      if (kept) this.all.update((now) => new Map([...Object.entries(kept), ...now]));
    });
  }

  /** The file's run window; null when none is marked. */
  async load(file: File): Promise<RunMarks | null> {
    await this.read;
    return this.all().get(fingerprint(file)) ?? null;
  }

  /** Keeps the file's run window (null: forgets it). */
  async save(file: File, marks: RunMarks | null): Promise<void> {
    await this.read;
    this.all.update((now) => {
      const next = new Map(now);
      if (marks) next.set(fingerprint(file), marks);
      else next.delete(fingerprint(file));
      return next;
    });
    await this.store.set(KEY, Object.fromEntries(this.all()));
  }
}
