import { inject, Injectable, signal } from '@angular/core';
import { FaintSetting } from '../../api';
import { BrowserStore } from './browser-store';
import { fingerprint } from './saved-reviews';

const KEY = 'faint-cutoffs';
const SKIPPED_KEY = 'faint-skipped';

/** The offset a cut-off takes when none is saved (python/server.py: `faint`). */
export const DEFAULT_OFFSET = 0.3;

/** Every recording's cut-off, by its file's fingerprint. */
type FaintIndex = Record<string, FaintSetting>;

/**
 * The user's faint-target cut-off for each recording opened in this browser, kept in it (IndexedDB) as the review
 * server keeps faint.json, and the recordings the cut-off queue skips (faint_skipped.json): the same file added again
 * finds them.
 */
@Injectable({ providedIn: 'root' })
export class SavedFaint {
  private readonly store = inject(BrowserStore);
  /** The cut-offs by fingerprint; read from the store once. */
  readonly all = signal<ReadonlyMap<string, FaintSetting>>(new Map());
  /** The skipped recordings' fingerprints. */
  readonly skipped = signal<ReadonlySet<string>>(new Set());
  readonly ready: Promise<void>;

  constructor() {
    this.ready = Promise.all([
      this.store.get<FaintIndex>(KEY).then((kept) => {
        if (kept) this.all.update((now) => new Map([...Object.entries(kept), ...now]));
      }),
      this.store.get<string[]>(SKIPPED_KEY).then((kept) => {
        if (kept) this.skipped.update((now) => new Set([...kept, ...now]));
      }),
    ]).then(() => undefined);
  }

  /** The file's cut-off: off at 0.3 when none is saved. */
  get(file: File): FaintSetting {
    return this.all().get(fingerprint(file)) ?? { on: false, offset: DEFAULT_OFFSET };
  }

  /** Keeps the file's cut-off. */
  async save(file: File, setting: FaintSetting): Promise<void> {
    await this.ready;
    this.all.update((now) => new Map(now).set(fingerprint(file), setting));
    await this.store.set(KEY, Object.fromEntries(this.all()));
  }

  /** Leaves the file out of the cut-off queue from now on. */
  async skip(file: File): Promise<void> {
    await this.ready;
    this.skipped.update((now) => new Set(now).add(fingerprint(file)));
    await this.store.set(SKIPPED_KEY, [...this.skipped()]);
  }
}
