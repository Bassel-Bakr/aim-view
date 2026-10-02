import { computed, Injectable, signal } from '@angular/core';
import { StatsCandidate } from '../../api';
import { parseStatsName, SAME_RUN_S, stampSeconds } from './stats-csv';

/** Stats files offered to pair a recording with, nearest its time first. */
const CANDIDATES = 40;

/** A stats file in the folder: its name, its scenario, when its run ended (stamp, and as seconds). */
export interface StatsEntry {
  name: string;
  scenario: string;
  stamp: string;
  seconds: number;
}

/** Not listed yet, being listed, or listed: its stats files known by scenario and time. */
export type FolderState = 'none' | 'listing' | 'ready';

/** Where the folder's files are read from: its handle, or the files of a folder input (this visit only). */
type Reader = (name: string) => Promise<File>;

/**
 * KovaaK's stats folder (FPSAimTrainer\stats), as KovaakFolders finds it: its files indexed by scenario and time
 * (as server.py's load_stats_index), so a recording finds its stats file by name and time, and the user can search
 * them by scenario.
 */
@Injectable({ providedIn: 'root' })
export class StatsFolder {
  readonly state = signal<FolderState>('none');
  readonly ready = computed(() => this.state() === 'ready');
  private byScenario = new Map<string, StatsEntry[]>();
  private reader: Reader | null = null;
  /** How many stats files the folder holds. */
  files = 0;

  /** Lists the folder and indexes its stats files. */
  async useFolder(dir: FileSystemDirectoryHandle): Promise<void> {
    this.state.set('listing');
    const names: string[] = [];
    for await (const [name, entry] of dir.entries()) {
      if (entry.kind === 'file') names.push(name);
    }
    this.index(names);
    this.reader = async (n) => (await dir.getFileHandle(n)).getFile();
  }

  /** The folder chosen as files (a folder input): read for this visit only. */
  async openFiles(files: readonly File[]): Promise<void> {
    const byName = new Map(files.map((f) => [f.name, f]));
    this.index([...byName.keys()]);
    this.reader = async (n) => {
      const f = byName.get(n);
      if (!f) throw new Error(`${n} is not in the stats folder`);
      return f;
    };
  }

  /** Indexes the stats files among the names, by scenario. */
  index(names: readonly string[]): void {
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
    this.files = files;
    this.state.set('ready');
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
