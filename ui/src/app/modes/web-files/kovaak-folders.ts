import { computed, inject, Injectable, signal } from '@angular/core';
import { BrowserStore } from './browser-store';
import { ScenarioFacts, ScenarioSource, windowsOrder } from './scenario-facts';
import { StatsFolder } from './stats-folder';

const ROOTS_KEY = 'kovaak-folders';

/** The folders of KovaaK's the review reads: its stats files, the user's scenarios, the workshop's scenarios. */
export type FolderRole = 'stats' | 'scenarios' | 'workshop';

/** A folder found, by its role. */
export type FoundFolders = Partial<Record<FolderRole, FileSystemDirectoryHandle>>;

/**
 * Where each folder sits below a folder the user picks, tried in order. The picked folder itself counts when its
 * name is the role's (stats, Scenarios, 824270): so the user can pick each folder, or steamapps (or Steam) for all.
 */
const PATHS: Record<FolderRole, string[][]> = {
  stats: [
    ['stats'],
    ['FPSAimTrainer', 'stats'],
    ['common', 'FPSAimTrainer', 'FPSAimTrainer', 'stats'],
  ],
  scenarios: [
    ['Saved', 'SaveGames', 'Scenarios'],
    ['FPSAimTrainer', 'Saved', 'SaveGames', 'Scenarios'],
    ['common', 'FPSAimTrainer', 'FPSAimTrainer', 'Saved', 'SaveGames', 'Scenarios'],
  ],
  workshop: [['workshop', 'content', '824270'], ['content', '824270'], ['824270']],
};
const NAMES: Record<FolderRole, string> = {
  stats: 'stats',
  scenarios: 'scenarios',
  workshop: '824270',
};
const STEAM = ['steamapps'];

/** What the user is told about the folders: what was found, and whether the browser needs leave to read them. */
export interface FoldersState {
  found: FolderRole[];
  ask: string[];
  refused: string | null;
  busy: boolean;
}

async function below(
  dir: FileSystemDirectoryHandle,
  path: string[],
): Promise<FileSystemDirectoryHandle | null> {
  let at = dir;
  for (const name of path) {
    try {
      at = await at.getDirectoryHandle(name);
    } catch {
      return null;
    }
  }
  return at;
}

/** The role folders inside (or at) a folder the user picked. */
export async function findFolders(root: FileSystemDirectoryHandle): Promise<FoundFolders> {
  const found: FoundFolders = {};
  for (const role of Object.keys(PATHS) as FolderRole[]) {
    if (root.name.toLowerCase() === NAMES[role]) {
      found[role] = root;
      continue;
    }
    for (const path of [...PATHS[role], ...PATHS[role].map((p) => [...STEAM, ...p])]) {
      const dir = await below(root, path);
      if (dir) {
        found[role] = dir;
        break;
      }
    }
  }
  return found;
}

/** The .sce files of a folder, as Python's glob lists them. */
async function sceFiles(dir: FileSystemDirectoryHandle, prefix: string): Promise<ScenarioSource[]> {
  const files: ScenarioSource[] = [];
  for await (const [name, entry] of dir.entries()) {
    if (entry.kind === 'file' && /\.sce$/i.test(name))
      files.push({
        path: `${prefix}/${name}`,
        name,
        file: () => (entry as FileSystemFileHandle).getFile(),
      });
  }
  return files.sort((a, b) => windowsOrder(a.name, b.name));
}

/** A workshop item: its folder's name (its id) and the folder. */
type WorkshopItem = [name: string, dir: FileSystemDirectoryHandle];

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

/** The workshop's scenario files: each item's folder, in order, and its .sce files. */
async function workshopFiles(dir: FileSystemDirectoryHandle): Promise<ScenarioSource[]> {
  const items: WorkshopItem[] = [];
  for await (const [name, entry] of dir.entries()) {
    if (entry.kind === 'directory') items.push([name, entry as FileSystemDirectoryHandle]);
  }
  items.sort((a, b) => windowsOrder(a[0], b[0]));
  const out: ScenarioSource[] = [];
  for (const [name, item] of items) out.push(...(await sceFiles(item, `workshop/${name}`)));
  return out;
}

/**
 * KovaaK's folders, opened by the user in the browser and remembered across visits (the browser asks once a visit
 * before reading them again): the stats files (StatsFolder) and the scenario files (ScenarioFacts) found in them.
 */
@Injectable({ providedIn: 'root' })
export class KovaakFolders {
  private readonly store = inject(BrowserStore);
  private readonly stats = inject(StatsFolder);
  private readonly scenarios = inject(ScenarioFacts);
  private roots: FileSystemDirectoryHandle[] = [];
  private readonly asking = signal<string[]>([]);
  private readonly refusal = signal<string | null>(null);
  private readonly busy = signal(false);
  /** Whether this browser has the folder picker (Chromium does; others choose a folder as files). */
  readonly picker =
    typeof window !== 'undefined' && typeof window.showDirectoryPicker === 'function';

  readonly state = computed<FoldersState>(() => ({
    found: [
      ...(this.stats.ready() ? (['stats'] as const) : []),
      ...(this.scenarios.sources().has('scenarios') ? (['scenarios'] as const) : []),
      ...(this.scenarios.sources().has('workshop') ? (['workshop'] as const) : []),
    ],
    ask: this.asking(),
    refused: this.refusal(),
    busy: this.busy() || this.scenarios.reading(),
  }));

  constructor() {
    void this.restore();
  }

  /** The folders from a visit before: read at once where the browser still lets them be, else waiting for leave. */
  private async restore(): Promise<void> {
    const roots =
      (await this.store.get<FileSystemDirectoryHandle[]>(ROOTS_KEY).catch(() => undefined)) ?? [];
    this.roots = roots;
    const ask: string[] = [];
    for (const root of roots) {
      const leave = await root
        .queryPermission({ mode: 'read' })
        .catch(() => 'prompt' as PermissionState);
      if (leave !== 'granted') ask.push(root.name);
    }
    this.asking.set(ask);
    if (!ask.length && roots.length) await this.use();
  }

  /** The user picks a folder (in a click): a KovaaK folder, or one above them. */
  async open(): Promise<void> {
    const pick = window.showDirectoryPicker;
    if (!pick) {
      this.refusal.set('This browser has no folder picker');
      return;
    }
    let root: FileSystemDirectoryHandle;
    try {
      root = await pick.call(window, { id: 'kovaak', mode: 'read' });
    } catch (e) {
      const closed = e instanceof DOMException && e.name === 'AbortError';
      this.refusal.set(closed ? 'The folder was not opened' : String(e));
      return;
    }
    this.refusal.set(null);
    this.roots = [...this.roots.filter((r) => r.name !== root.name), root];
    await this.store.set(ROOTS_KEY, this.roots).catch(() => undefined);
    await this.use();
  }

  /** The user gives leave again to read the remembered folders (in a click). */
  async allow(): Promise<void> {
    for (const root of this.roots) await root.requestPermission({ mode: 'read' });
    this.asking.set([]);
    await this.use();
  }

  /**
   * Folders chosen as files (a folder input), where the folder picker cannot open them (Chrome refuses folders under
   * Program Files): the stats files for this visit only, the scenarios' facts kept.
   */
  async openFiles(files: readonly File[]): Promise<void> {
    const chosen = sortChosen(files);
    if (!chosen.stats.length && !chosen.scenarios.length)
      throw new Error("No stats or scenario files of KovaaK's in the folder chosen");
    if (chosen.stats.length) await this.stats.openFiles(chosen.stats);
    if (chosen.scenarios.length) await this.scenarios.read(chosen.scenarios);
  }

  /** Finds the role folders in the picked folders, then reads them: the stats index, then the scenarios' facts. */
  private async use(): Promise<void> {
    this.busy.set(true);
    try {
      const found: FoundFolders = {};
      for (const root of this.roots) Object.assign(found, await findFolders(root));
      if (found.stats) await this.stats.useFolder(found.stats);
      const sources = [
        ...(found.scenarios ? await sceFiles(found.scenarios, 'scenarios') : []),
        ...(found.workshop ? await workshopFiles(found.workshop) : []),
      ];
      if (sources.length) await this.scenarios.read(sources);
    } finally {
      this.busy.set(false);
    }
  }
}
