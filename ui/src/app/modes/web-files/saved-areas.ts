import { inject, Injectable, signal } from '@angular/core';
import { AreaBox, AreaSet } from '../../api';
import { FoundArea } from '../wasm/area-finder-messages';
import { kovobsLayout } from './area-kinds';
import { BrowserStore, StoreEntry } from './browser-store';
import { fingerprint } from './saved-reviews';

const KEY = 'exclude-areas';
const ADDED_KEY = 'exclude-areas-added';
const FOUND_KEY = 'exclude-areas-found';

/** Every recording's saved areas, by its file's fingerprint. */
type AreasIndex = Record<string, AreaBox[]>;

/**
 * What the area finder had found in a recording when the user saved its areas (areas.json on the review server): the
 * recording's name in the area examples (exampleRec), its file's name without the extension, and the areas.
 */
export interface FoundWhenSaved {
  rec: string;
  name: string;
  found: FoundArea[];
}

/** Every recording's found areas, by its file's fingerprint. */
type FoundIndex = Record<string, FoundWhenSaved>;

/**
 * A recording the user saved areas for, as the area finder compares layouts (src/areas.rs: Labelled): its name, the
 * areas found in it and the areas saved.
 */
export interface LabelledRecording {
  rec: string;
  found: FoundArea[];
  saved: AreaBox[];
}

/**
 * The excluded areas saved for each recording opened in this browser, kept in it (IndexedDB) as the review server
 * keeps exclude.json: the same file added again finds its areas. Also the areas last saved for an added recording
 * (exclude_uploads.json there), which an added recording without areas of its own starts from, and what the area
 * finder had found in each recording saved (areas.json there), so Find areas can copy a recording with the same layout.
 */
@Injectable({ providedIn: 'root' })
export class SavedAreas {
  private readonly store = inject(BrowserStore);
  /** The areas by fingerprint; read from the store once. */
  readonly all = signal<ReadonlyMap<string, AreaBox[]>>(new Map());
  /** The areas last saved for an added recording; null when none were. */
  readonly lastAdded = signal<AreaBox[] | null>(null);
  /** The found areas of the recordings saved, by fingerprint. */
  private readonly found = signal<ReadonlyMap<string, FoundWhenSaved>>(new Map());
  private readonly read: Promise<void>;

  constructor() {
    this.read = Promise.all([
      this.store.get<AreasIndex>(KEY),
      this.store.get<AreaBox[]>(ADDED_KEY),
      this.store.get<FoundIndex>(FOUND_KEY),
    ]).then(([kept, added, found]) => {
      if (kept) this.all.update((now) => new Map([...Object.entries(kept), ...now]));
      if (added) this.lastAdded.update((now) => now ?? added);
      if (found) this.found.update((now) => new Map([...Object.entries(found), ...now]));
    });
  }

  /** The file's saved areas; null when none are saved. */
  async load(file: File): Promise<AreaBox[] | null> {
    await this.read;
    return this.all().get(fingerprint(file)) ?? null;
  }

  /**
   * The areas a recording's review ignores: saved for it, else for an added recording (added) the ones last saved for
   * one, else KovOBS's layout.
   */
  async areasOf(file: File, added: boolean): Promise<AreaSet> {
    await this.read;
    const saved = this.all().get(fingerprint(file));
    if (saved) return { boxes: saved, source: 'saved' };
    const last = added ? this.lastAdded() : null;
    if (last) return { boxes: last, source: 'last upload' };
    return { boxes: kovobsLayout(), source: 'kovobs' };
  }

  /**
   * Keeps the file's areas; added: it is a recording added from this computer, whose areas the next one starts from;
   * found: what the area finder had found in it (null: it had not run).
   */
  async save(
    file: File,
    boxes: AreaBox[],
    added: boolean,
    found: FoundWhenSaved | null,
  ): Promise<void> {
    await this.read;
    const key = fingerprint(file);
    this.all.update((now) => new Map(now).set(key, boxes));
    if (added) this.lastAdded.set(boxes);
    if (found) this.found.update((now) => new Map(now).set(key, found));
    const entries: StoreEntry[] = [[KEY, Object.fromEntries(this.all())]];
    if (added) entries.push([ADDED_KEY, boxes]);
    if (found) entries.push([FOUND_KEY, Object.fromEntries(this.found())]);
    await this.store.setMany(entries);
  }

  /** The other recordings with saved areas and found areas, but those left out (by their exampleRec). */
  async labelled(but: File, leftOut: ReadonlySet<string>): Promise<LabelledRecording[]> {
    await this.read;
    const skip = fingerprint(but);
    const out: LabelledRecording[] = [];
    for (const [key, f] of this.found()) {
      const saved = this.all().get(key);
      if (key !== skip && saved && !leftOut.has(f.rec)) {
        out.push({ rec: f.name, found: f.found, saved });
      }
    }
    return out;
  }
}
