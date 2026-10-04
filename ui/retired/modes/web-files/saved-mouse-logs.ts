import { inject, Injectable, signal } from '@angular/core';
import { BrowserStore } from './browser-store';
import { fingerprint } from './saved-reviews';

const INDEX = 'mouse-logs';
const LOG = 'mouse-log|';

/** The kept logs' file names, by their recording's fingerprint. */
type MouseLogIndex = Record<string, string>;

/**
 * The mouse log added for each recording opened in this browser, kept in it (IndexedDB) with the recording's
 * fingerprint, so the same file added again finds its log. Without IndexedDB (tests) they are kept for the visit.
 */
@Injectable({ providedIn: 'root' })
export class SavedMouseLogs {
  private readonly store = inject(BrowserStore);
  /** The logs' file names by fingerprint; read from the store once. */
  private readonly names = signal<ReadonlyMap<string, string>>(new Map());
  private readonly bytes = new Map<string, ArrayBuffer>();
  private readonly read: Promise<void>;

  constructor() {
    this.read = this.store.get<MouseLogIndex>(INDEX).then((kept) => {
      if (kept) this.names.update((now) => new Map([...Object.entries(kept), ...now]));
    });
  }

  /** The name of the log kept for the recording's file; null when none (a signal read: it updates). */
  name(file: File): string | null {
    return this.names().get(fingerprint(file)) ?? null;
  }

  /** The log kept for the recording's file. */
  async load(file: File): Promise<ArrayBuffer | null> {
    await this.read;
    const key = fingerprint(file);
    const kept = this.bytes.get(key) ?? (await this.store.get<ArrayBuffer>(LOG + key)) ?? null;
    if (kept) this.bytes.set(key, kept);
    return kept;
  }

  /** Keeps a log for the recording's file, in place of any before it. */
  async save(file: File, name: string, log: ArrayBuffer): Promise<void> {
    await this.read;
    const key = fingerprint(file);
    this.bytes.set(key, log);
    this.names.update((now) => new Map(now).set(key, name));
    await this.store.setMany([
      [LOG + key, log],
      [INDEX, Object.fromEntries(this.names())],
    ]);
  }

  /** Forgets the log kept for the recording's file. */
  async forget(file: File): Promise<void> {
    await this.read;
    const key = fingerprint(file);
    this.bytes.delete(key);
    this.names.update((now) => {
      const next = new Map(now);
      next.delete(key);
      return next;
    });
    await this.store.remove(LOG + key);
    await this.store.set(INDEX, Object.fromEntries(this.names()));
  }
}
