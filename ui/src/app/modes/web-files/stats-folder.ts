import { computed, inject, Injectable, signal } from '@angular/core';
import { StatsCandidate } from '../../api';
import { BrowserStore } from './browser-store';
import { parseStatsName, SAME_RUN_S, stampSeconds } from './stats-csv';

const HANDLE_KEY = 'stats-folder';
/** Stats files offered to pair a recording with, nearest its time first. */
const CANDIDATES = 40;

/** A stats file in the folder: its name, its scenario, when its run ended (stamp, and as seconds). */
export interface StatsEntry {
  name: string;
  scenario: string;
  stamp: string;
  seconds: number;
}

/** Never opened. */
export interface FolderNone {
  kind: 'none';
}

/** Remembered from a visit before; the browser needs the user's leave to read it again. */
export interface FolderAsk {
  kind: 'ask';
  name: string;
}

/** Being listed. */
export interface FolderListing {
  kind: 'listing';
  name: string;
}

/** Listed: its stats files are known by scenario and time. */
export interface FolderReady {
  kind: 'ready';
  name: string;
  files: number;
}

/** The browser would not open it with its folder picker; it can still be chosen as a folder of files. */
export interface FolderRefused {
  kind: 'refused';
  error: string;
}

export type FolderState = FolderNone | FolderAsk | FolderListing | FolderReady | FolderRefused;

/** Where the folder's files are read from: a remembered handle, or the files of a folder input (this visit only). */
type Reader = (name: string) => Promise<File>;

/**
 * KovaaK's stats folder (FPSAimTrainer\stats), opened by the user in the browser and remembered across visits: its
 * files indexed by scenario and time (as server.py's load_stats_index), so a recording finds its stats file by name and
 * time, and the user can search them by scenario.
 */
@Injectable({ providedIn: 'root' })
export class StatsFolder {
  private readonly store = inject(BrowserStore);
  readonly state = signal<FolderState>({ kind: 'none' });
  readonly ready = computed(() => this.state().kind === 'ready');
  /** Whether this browser has the folder picker (Chromium does; others choose the folder as files). */
  readonly picker =
    typeof window !== 'undefined' && typeof window.showDirectoryPicker === 'function';
  private byScenario = new Map<string, StatsEntry[]>();
  private reader: Reader | null = null;
  private handle: FileSystemDirectoryHandle | null = null;

  constructor() {
    void this.restore();
  }

  /** The folder from a visit before: listed at once when the browser still lets it be read, else waiting for leave. */
  private async restore(): Promise<void> {
    const handle = await this.store
      .get<FileSystemDirectoryHandle>(HANDLE_KEY)
      .catch(() => undefined);
    if (!handle) return;
    this.handle = handle;
    const leave = await handle
      .queryPermission({ mode: 'read' })
      .catch(() => 'prompt' as PermissionState);
    if (leave === 'granted') await this.listHandle(handle);
    else this.state.set({ kind: 'ask', name: handle.name });
  }

  /** The user picks the folder (in a click). */
  async open(): Promise<void> {
    const pick = window.showDirectoryPicker;
    if (!pick) {
      this.state.set({ kind: 'refused', error: 'This browser has no folder picker' });
      return;
    }
    let handle: FileSystemDirectoryHandle;
    try {
      handle = await pick.call(window, { id: 'kovaak-stats', mode: 'read' });
    } catch (e) {
      // closed, or refused by the browser (it says why itself): the same error either way, so the picker stays
      // and the folder can also be chosen as files
      const closed = e instanceof DOMException && e.name === 'AbortError';
      this.state.set({ kind: 'refused', error: closed ? 'The folder was not opened' : String(e) });
      return;
    }
    this.handle = handle;
    await this.store.set(HANDLE_KEY, handle).catch(() => undefined);
    await this.listHandle(handle);
  }

  /** The user gives leave again for the remembered folder (in a click). */
  async allow(): Promise<void> {
    if (!this.handle) return this.open();
    const leave = await this.handle.requestPermission({ mode: 'read' });
    if (leave === 'granted') await this.listHandle(this.handle);
  }

  /** The folder chosen as files (a folder input): read for this visit only. */
  async openFiles(files: readonly File[]): Promise<void> {
    const byName = new Map(files.map((f) => [f.name, f]));
    const name = (files[0]?.webkitRelativePath ?? '').split('/')[0] || 'stats';
    this.index(name, [...byName.keys()]);
    this.reader = async (n) => {
      const f = byName.get(n);
      if (!f) throw new Error(`${n} is not in the stats folder`);
      return f;
    };
  }

  private async listHandle(handle: FileSystemDirectoryHandle): Promise<void> {
    this.state.set({ kind: 'listing', name: handle.name });
    const names: string[] = [];
    for await (const [name, entry] of handle.entries()) {
      if (entry.kind === 'file') names.push(name);
    }
    this.index(handle.name, names);
    this.reader = async (n) => (await handle.getFileHandle(n)).getFile();
  }

  /** Indexes the stats files among the names, by scenario. */
  index(folder: string, names: readonly string[]): void {
    const by = new Map<string, StatsEntry[]>();
    let files = 0;
    for (const name of names) {
      const p = parseStatsName(name);
      const seconds = p && stampSeconds(p.stamp);
      if (!p || seconds === null) continue;
      const list = by.get(p.scenario) ?? [];
      list.push({ name, scenario: p.scenario, stamp: p.stamp, seconds });
      by.set(p.scenario, list);
      files++;
    }
    this.byScenario = by;
    this.state.set({ kind: 'ready', name: folder, files });
  }

  /** The stats file of the same scenario within five seconds of the time, the nearest (server.py: stats_for). */
  find(scenario: string, seconds: number | null): StatsEntry | null {
    if (seconds === null) return null;
    let best: StatsEntry | null = null;
    for (const e of this.byScenario.get(scenario) ?? []) {
      const d = Math.abs(e.seconds - seconds);
      if (d > SAME_RUN_S) continue;
      if (
        !best ||
        d < Math.abs(best.seconds - seconds) ||
        (d === Math.abs(best.seconds - seconds) && e.name < best.name)
      )
        best = e;
    }
    return best;
  }

  /**
   * Stats files to pair with, nearest the time first: the scenario's (its name, any case), or with a query those of
   * every scenario whose name holds it (server.py: stats_info).
   */
  candidates(scenario: string, query: string | null, seconds: number): StatsCandidate[] {
    const text = (query ?? scenario).trim().toLowerCase();
    const all: StatsEntry[] = [];
    for (const [s, list] of this.byScenario) {
      const name = s.toLowerCase();
      if (query === null ? name === text : name.includes(text)) all.push(...list);
    }
    return all
      .sort(
        (a, b) =>
          Math.abs(a.seconds - seconds) - Math.abs(b.seconds - seconds) || a.seconds - b.seconds,
      )
      .slice(0, CANDIDATES)
      .map((e) => ({
        name: e.name,
        scenario: e.scenario,
        stamp: e.stamp,
        off: Math.round((e.seconds - seconds) * 10) / 10,
      }));
  }

  /** A stats file's contents. */
  async read(name: string): Promise<File> {
    if (!this.reader) throw new Error("KovaaK's stats folder is not open");
    return this.reader(name);
  }
}
