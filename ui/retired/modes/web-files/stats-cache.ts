import { inject, Injectable } from '@angular/core';
import { ItemCount } from '../../platform/recording-source';
import { BrowserStore, StoreEntry } from './browser-store';

const INDEX_KEY = 'stats-index';
const FILE_KEY = 'stats-file:';
/** Stats files stored together in one transaction. */
const BATCH = 500;
/** The index is saved again after this many files, so a reload keeps what was stored by then. */
const INDEX_EVERY = 5000;

/** A stats file as kept: its name, size and time (a file chosen again with the same ones is not stored again). */
export type KeptFile = [name: string, size: number, modified: number];

/**
 * KovaaK's stats files, copied into this browser (IndexedDB) when the stats folder is chosen as files: Chrome does not
 * let a page keep a folder under Program Files, so the copy is what a later visit reads. Choosing the folder again
 * copies only the files that are new or changed.
 */
@Injectable({ providedIn: 'root' })
export class StatsCache {
  private readonly store = inject(BrowserStore);

  /** The names of the stats files kept, or null when none are. */
  async names(): Promise<string[] | null> {
    const index = await this.store.get<KeptFile[]>(INDEX_KEY).catch(() => undefined);
    return index?.length ? index.map((k) => k[0]) : null;
  }

  /** A kept stats file. */
  async read(name: string): Promise<File> {
    const blob = await this.store.get<Blob>(FILE_KEY + name);
    if (!blob) throw new Error(`${name} is not kept in this browser`);
    return new File([blob], name);
  }

  /** Keeps the stats files that are not kept yet, or changed, a batch at a time (counted). */
  async keep(files: readonly File[], counted: (count: ItemCount) => void): Promise<void> {
    const index = (await this.store.get<KeptFile[]>(INDEX_KEY).catch(() => undefined)) ?? [];
    const known = new Map(index.map((k) => [k[0], k]));
    const fresh = files.filter((f) => {
      const k = known.get(f.name);
      return !k || k[1] !== f.size || k[2] !== f.lastModified;
    });
    for (let i = 0; i < fresh.length; i += BATCH) {
      counted({ done: i, total: fresh.length });
      const batch = fresh.slice(i, i + BATCH);
      await this.store.setMany(batch.map((f): StoreEntry => [FILE_KEY + f.name, f]));
      for (const f of batch) known.set(f.name, [f.name, f.size, f.lastModified]);
      if ((i + BATCH) % INDEX_EVERY === 0 || i + BATCH >= fresh.length)
        await this.store.set(INDEX_KEY, [...known.values()]);
    }
  }
}
