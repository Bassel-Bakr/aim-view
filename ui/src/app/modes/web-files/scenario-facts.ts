import { computed, inject, Injectable, signal } from '@angular/core';
import { Kind, ScenarioInfo } from '../../api';
import { ItemCount } from '../../platform/recording-source';
import { CoreModule } from '../wasm/core-module';
import { BrowserStore } from './browser-store';

const CACHE_KEY = 'scenario-facts';
const MAP_DATA = '[Map Data]';
const FIRST_READ = 64 * 1024;

/** A scenario file to read: its path (for the cache), its name, and how to get its contents. */
export interface ScenarioSource {
  path: string;
  name: string;
  file: () => Promise<File>;
}

/** A file's facts as cached: the facts, and the file's size and time when they were read. */
export interface CachedFacts {
  size: number;
  modified: number;
  facts: ScenarioInfo;
}

/** The cache: facts by file path ("scenarios/<name>" or "workshop/<item>/<name>"). */
export type FactsCache = Record<string, CachedFacts>;

/** Where a scenario file came from: the user's scenarios folder, or the workshop's. */
export type FactsSource = 'scenarios' | 'workshop';

function sourceOf(path: string): FactsSource {
  return path.startsWith('workshop/') ? 'workshop' : 'scenarios';
}

/** Cache paths in the order Python's scenario_facts reads the files: the user's, then the workshop's by item. */
function readOrder(a: string, b: string): number {
  if (sourceOf(a) !== sourceOf(b)) return sourceOf(a) === 'scenarios' ? -1 : 1;
  const pa = a.split('/');
  const pb = b.split('/');
  for (let k = 1; k < Math.min(pa.length, pb.length); k++) {
    const c = windowsOrder(pa[k], pb[k]);
    if (c) return c;
  }
  return pa.length - pb.length;
}

/** Names in the order Windows lists a folder (NTFS: by name, letters compared as capitals), as Python's glob has them. */
export function windowsOrder(a: string, b: string): number {
  const x = a.toUpperCase();
  const y = b.toUpperCase();
  return x < y ? -1 : x > y ? 1 : 0;
}

/** The file's text up to "[Map Data]" (the part the facts come from), read in growing pieces. */
async function header(file: File): Promise<string> {
  for (let n = FIRST_READ; ; n *= 4) {
    const text = await file.slice(0, n).text();
    const at = text.indexOf(MAP_DATA);
    if (at >= 0) return text.slice(0, at);
    if (n >= file.size) return text;
  }
}

/**
 * Each scenario's facts (src/scenario.rs, through the core), read from KovaaK's scenario files: the user's own
 * (Saved\SaveGames\Scenarios), then the workshop's (workshop\content\824270), later files winning as in Python's
 * scenario_facts. Facts are kept by file in this browser: a later visit has them before any folder is opened again,
 * and reads only new or changed files.
 */
@Injectable({ providedIn: 'root' })
export class ScenarioFacts {
  private readonly core = inject(CoreModule);
  private readonly store = inject(BrowserStore);
  readonly byName = signal<ReadonlyMap<string, ScenarioInfo>>(new Map());
  readonly reading = signal(false);
  /** How many of the files being read are read. */
  readonly progress = signal<ItemCount | null>(null);
  readonly count = computed(() => this.byName().size);
  /** The folders the facts came from (this visit or one before). */
  readonly sources = signal<ReadonlySet<FactsSource>>(new Set());

  constructor() {
    void this.restore();
  }

  /** The facts kept from a visit before, until a folder is read. */
  private async restore(): Promise<void> {
    const cache = await this.cached();
    if (!this.byName().size) this.use(cache);
  }

  private async cached(): Promise<FactsCache> {
    return (await this.store.get<FactsCache>(CACHE_KEY).catch(() => undefined)) ?? {};
  }

  /** The facts by name from every file kept, read in Python's order. */
  private use(cache: FactsCache): void {
    const by = new Map<string, ScenarioInfo>();
    const from = new Set<FactsSource>();
    for (const path of Object.keys(cache).sort(readOrder)) {
      const name = path.slice(path.lastIndexOf('/') + 1).replace(/\.sce$/i, '');
      by.set(name.toLowerCase(), cache[path].facts);
      from.add(sourceOf(path));
    }
    this.byName.set(by);
    this.sources.set(from);
  }

  /** The scenario's facts, by its name (any case), or null when no file of it was read. */
  get(scenario: string): ScenarioInfo | null {
    return this.byName().get(scenario.toLowerCase()) ?? null;
  }

  kind(scenario: string): Kind | null {
    return this.get(scenario)?.kind ?? null;
  }

  /**
   * Reads a folder's files (paths "scenarios/<name>" or "workshop/<item>/<name>"): they replace what was kept from the
   * same folder, and the other folder's facts stay.
   */
  async read(sources: readonly ScenarioSource[]): Promise<void> {
    this.reading.set(true);
    try {
      const cache = await this.cached();
      const replaced = new Set(sources.map((s) => sourceOf(s.path)));
      const next: FactsCache = {};
      for (const [path, hit] of Object.entries(cache))
        if (!replaced.has(sourceOf(path))) next[path] = hit;
      for (const [k, s] of sources.entries()) {
        this.progress.set({ done: k, total: sources.length });
        const file = await s.file();
        const hit = cache[s.path];
        const facts =
          hit && hit.size === file.size && hit.modified === file.lastModified
            ? hit.facts
            : await this.core.scenarioFacts(await header(file));
        next[s.path] = { size: file.size, modified: file.lastModified, facts };
      }
      this.use(next);
      await this.store.set(CACHE_KEY, next).catch(() => undefined);
    } finally {
      this.reading.set(false);
      this.progress.set(null);
    }
  }
}
