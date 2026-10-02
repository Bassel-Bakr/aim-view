import { computed, inject, Injectable, signal } from '@angular/core';
import { ItemCount } from '../../platform/recording-source';
import { ScenarioFacts, ScenarioSource, windowsOrder } from './scenario-facts';
import { StatsCache } from './stats-cache';
import { StatsFolder } from './stats-folder';

/** The folders of KovaaK's the review reads: its stats files, the user's scenarios, the workshop's scenarios. */
export type FolderRole = 'stats' | 'scenarios' | 'workshop';

/** What the user is told about the folders: which are open, and whether they are being read. */
export interface FoldersState {
  found: FolderRole[];
  busy: boolean;
}

/** Files chosen as a folder (a folder input), sorted by what their paths make them: stats files, and scenario files. */
export interface ChosenFiles {
  stats: File[];
  scenarios: ScenarioSource[];
}

/**
 * Sorts the files of a folder chosen as files: a .csv in a folder named stats is a stats file; a .sce in a folder named
 * Scenarios is one of the user's scenarios, and one in an item's folder inside 824270 the workshop's. So the user can
 * choose steamapps, FPSAimTrainer, or each folder.
 */
export function sortChosen(files: readonly File[]): ChosenFiles {
  const out: ChosenFiles = { stats: [], scenarios: [] };
  for (const f of files) {
    const parts = (f.webkitRelativePath || f.name).split('/');
    const at = (k: number) => (parts[parts.length - 1 - k] ?? '').toLowerCase();
    if (/\.csv$/i.test(f.name) && at(1) === 'stats') out.stats.push(f);
    else if (/\.sce$/i.test(f.name) && at(1) === 'scenarios')
      out.scenarios.push({ path: `scenarios/${f.name}`, name: f.name, file: async () => f });
    else if (/\.sce$/i.test(f.name) && at(2) === '824270')
      out.scenarios.push({
        path: `workshop/${parts[parts.length - 2]}/${f.name}`,
        name: f.name,
        file: async () => f,
      });
  }
  const order = (a: ScenarioSource, b: ScenarioSource) => {
    const [pa, pb] = [a.path.split('/'), b.path.split('/')];
    if (pa[0] !== pb[0]) return pa[0] === 'scenarios' ? -1 : 1;
    return windowsOrder(pa[1], pb[1]) || windowsOrder(pa[2] ?? '', pb[2] ?? '');
  };
  out.scenarios.sort(order);
  return out;
}

/**
 * KovaaK's folders, chosen by the user as files (a folder input: Chrome's folder picker refuses folders under Program
 * Files, where KovaaK's is): the stats files (StatsFolder), which the browser keeps a copy of (StatsCache), and the
 * scenario files (ScenarioFacts), whose facts it keeps.
 */
@Injectable({ providedIn: 'root' })
export class KovaakFolders {
  private readonly stats = inject(StatsFolder);
  private readonly scenarios = inject(ScenarioFacts);
  private readonly cache = inject(StatsCache);
  private readonly reading = signal(false);
  /** How many of the stats files being copied into this browser are copied. */
  readonly keeping = signal<ItemCount | null>(null);
  /** The stats files kept from a visit before are in use (or there are none). */
  readonly restored = this.restore();

  readonly state = computed<FoldersState>(() => ({
    found: [
      ...(this.stats.ready() ? (['stats'] as const) : []),
      ...(this.scenarios.sources().has('scenarios') ? (['scenarios'] as const) : []),
      ...(this.scenarios.sources().has('workshop') ? (['workshop'] as const) : []),
    ],
    busy: this.reading() || this.scenarios.reading(),
  }));

  private async restore(): Promise<void> {
    const names = await this.cache.names().catch(() => null);
    if (names && !this.stats.ready()) this.stats.useKept(names, (name) => this.cache.read(name));
  }

  /** Reads the folders chosen as files: the stats files, then the scenarios' facts; then keeps the stats files. */
  async openFiles(files: readonly File[]): Promise<void> {
    const chosen = sortChosen(files);
    if (!chosen.stats.length && !chosen.scenarios.length)
      throw new Error("No stats or scenario files of KovaaK's in the folder chosen");
    this.reading.set(true);
    try {
      if (chosen.stats.length) await this.stats.openFiles(chosen.stats);
      if (chosen.scenarios.length) await this.scenarios.read(chosen.scenarios);
    } finally {
      this.reading.set(false);
    }
    if (chosen.stats.length) void this.keep(chosen.stats);
  }

  /** Copies the stats files into this browser, so a later visit has them without choosing the folder. */
  private async keep(files: readonly File[]): Promise<void> {
    try {
      await this.cache.keep(files, (count) => this.keeping.set(count));
    } finally {
      this.keeping.set(null);
    }
  }
}
