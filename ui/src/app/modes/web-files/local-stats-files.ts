import { computed, inject, Injectable, resource, ResourceRef } from '@angular/core';
import { Job, StatsChange, StatsChoice, StatsHow, StatsPairing } from '../../api';
import { StatsFiles, StatsSetup } from '../../platform/stats-files';
import { LocalFiles, localRecording } from './local-files';
import { StatsCsv, stampSeconds, statsSummary } from './stats-csv';
import { StatsFolder } from './stats-folder';

/** What a pairing is read from: the recording, its stats file and how it came, the search, the folder's state. */
export interface LocalPairingParams {
  id: string;
  stats: StatsCsv | null;
  how: StatsHow;
  query: string | null;
  ready: boolean;
}

const NO_JOB: Job = { stage: 'none' };

/**
 * The stats files of recordings opened in the browser: found in KovaaK's stats folder by name and time (once the user
 * opens the folder; the browser remembers it), picked from its files by scenario, or a .csv chosen from this computer.
 * All read in the browser.
 */
@Injectable({ providedIn: 'root' })
export class LocalStatsFiles implements StatsFiles {
  private readonly local = inject(LocalFiles);
  private readonly folder = inject(StatsFolder);
  readonly searches = this.folder.ready;

  readonly setup = computed<StatsSetup | null>(() => {
    const state = this.folder.state();
    const open = async () => {
      await this.folder.open();
      await this.local.findAllStats();
    };
    const files = async (list: File[]) => {
      await this.folder.openFiles(list);
      await this.local.findAllStats();
    };
    switch (state.kind) {
      case 'ready':
      case 'listing':
        return null;
      case 'ask':
        return {
          label: `Allow reading ${state.name} again`,
          detail:
            'The browser remembers the stats folder, and asks once per visit before reading it again.',
          run: async () => {
            await this.folder.allow();
            await this.local.findAllStats();
          },
          files: null,
        };
      case 'refused':
        return {
          label: "Open KovaaK's stats folder",
          detail: `${state.error}. You can try again, or choose the folder as files: then it is read for this visit only.`,
          run: open,
          files,
        };
      default:
        return {
          label: "Open KovaaK's stats folder",
          detail:
            'FPSAimTrainer\\stats in the game folder: each run finds its stats file by its scenario and time. The ' +
            'browser remembers the folder.',
          run: open,
          files: this.folder.picker ? null : files,
        };
    }
  });

  pairing(
    id: () => string | undefined,
    query: () => string | null,
  ): ResourceRef<StatsPairing | undefined> {
    return resource({
      params: (): LocalPairingParams | undefined => {
        const at = id();
        const f = at === undefined ? null : this.local.find(at);
        if (!f) return undefined;
        return {
          id: f.id,
          stats: f.stats,
          how: f.statsHow,
          query: query(),
          ready: this.folder.ready(),
        };
      },
      loader: async ({ params }) => {
        const f = this.local.find(params.id);
        const r = f ? localRecording(f) : null;
        const seconds = r ? stampSeconds(r.stamp) : null;
        return {
          file: params.stats?.name ?? null,
          how: params.how,
          scenario: r?.scenario ?? '',
          candidates:
            params.ready && r && seconds !== null
              ? this.folder.candidates(r.scenario, params.query, seconds)
              : [],
          facts: params.stats ? statsSummary(params.stats) : undefined,
        };
      },
    });
  }

  /** One of the folder's stats files, none, or found by name and time again. */
  async choose(id: string, choice: StatsChoice): Promise<StatsChange> {
    if ('auto' in choice) return { job: NO_JOB, stats: await this.local.findStats(id) };
    if (choice.file === null) {
      this.local.unpair(id);
      return { job: NO_JOB, stats: false };
    }
    if (!(await this.local.pair(id, await this.folder.read(choice.file), 'picked')))
      throw new Error(`${choice.file} is not one of KovaaK's stats files`);
    return { job: NO_JOB, stats: true };
  }

  async pairFile(id: string, file: File): Promise<StatsChange> {
    if (!(await this.local.pair(id, file)))
      throw new Error(`${file.name} is not one of KovaaK's stats files`);
    return { job: NO_JOB, stats: true };
  }
}
