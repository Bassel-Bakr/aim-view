import { computed, inject, Injectable, signal } from '@angular/core';
import { Kind, ScenarioInfo } from '../../api';
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

/** The cache: facts by file path. */
export type FactsCache = Record<string, CachedFacts>;

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
 * scenario_facts. Facts are cached by file in this browser, so a later visit reads only new or changed files.
 */
@Injectable({ providedIn: 'root' })
export class ScenarioFacts {
  private readonly core = inject(CoreModule);
  private readonly store = inject(BrowserStore);
  readonly byName = signal<ReadonlyMap<string, ScenarioInfo>>(new Map());
  readonly reading = signal(false);
  readonly count = computed(() => this.byName().size);

  /** The scenario's facts, by its name (any case), or null when no file of it was read. */
  get(scenario: string): ScenarioInfo | null {
    return this.byName().get(scenario.toLowerCase()) ?? null;
  }

  kind(scenario: string): Kind | null {
    return this.get(scenario)?.kind ?? null;
  }

  /** Reads the files in the order given (later files win). */
  async read(sources: readonly ScenarioSource[]): Promise<void> {
    this.reading.set(true);
    try {
      const cache = (await this.store.get<FactsCache>(CACHE_KEY).catch(() => undefined)) ?? {};
      const next: FactsCache = {};
      const by = new Map<string, ScenarioInfo>();
      for (const s of sources) {
        const file = await s.file();
        const hit = cache[s.path];
        const facts =
          hit && hit.size === file.size && hit.modified === file.lastModified
            ? hit.facts
            : await this.core.scenarioFacts(await header(file));
        next[s.path] = { size: file.size, modified: file.lastModified, facts };
        by.set(s.name.replace(/\.sce$/i, '').toLowerCase(), facts);
      }
      this.byName.set(by);
      await this.store.set(CACHE_KEY, next).catch(() => undefined);
    } finally {
      this.reading.set(false);
    }
  }
}
