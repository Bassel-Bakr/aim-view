import { computed, inject, Injectable, resource, ResourceRef } from '@angular/core';
import { Job, StatsChange, StatsChoice, StatsHow, StatsPairing } from '../../api';
import { StatsFiles, StatsSetup } from '../../platform/stats-files';
import { LocalFiles, localRecording } from './local-files';
import { StatsCsv, stampSeconds, statsSummary } from './stats-csv';
import { KovaakFolders } from './kovaak-folders';
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

/** The folders as the user is told about them. */
const ROLE_NAMES = {
  stats: 'the stats files',
  scenarios: 'your scenarios',
  workshop: "the workshop's scenarios",
} as const;

/** The stats folder chosen as files (where the browser's picker cannot open it), then the runs' stats found. */
function readStatsFiles(
  folders: KovaakFolders,
  local: LocalFiles,
): (files: File[]) => Promise<void> {
  return async (files) => {
    await folders.openStatsFiles(files);
    await local.findAllStats();
  };
}

/**
 * The stats files of recordings opened in the browser: found in KovaaK's stats folder by name and time (once the user
 * opens the folder; the browser remembers it), picked from its files by scenario, or a .csv chosen from this computer.
 * All read in the browser.
 */
@Injectable({ providedIn: 'root' })
export class LocalStatsFiles implements StatsFiles {
  private readonly local = inject(LocalFiles);
  private readonly folder = inject(StatsFolder);
  private readonly folders = inject(KovaakFolders);
  readonly searches = this.folder.ready;

  readonly setup = computed<StatsSetup | null>(() => {
    const state = this.folders.state();
    const then = (step: () => Promise<void>) => async () => {
      await step();
      await this.local.findAllStats();
    };
    if (state.busy) return null;
    if (state.ask.length) {
      return {
        label: `Allow reading ${state.ask.join(', ')} again`,
        detail:
          "The browser remembers KovaaK's folders, and asks once per visit before reading them again.",
        run: then(() => this.folders.allow()),
        files: null,
      };
    }
    const missing = (['stats', 'scenarios', 'workshop'] as const).filter(
      (r) => !state.found.includes(r),
    );
    if (!missing.length) return null;
    const what = missing.map((r) => ROLE_NAMES[r]).join(', ');
    return {
      label: state.found.length ? "Open more of KovaaK's folders" : "Open KovaaK's folders",
      detail:
        (state.refused ? `${state.refused}. ` : '') +
        `Missing: ${what}. Pick steamapps (in the Steam folder) to give everything at once, or each folder: ` +
        String.raw`FPSAimTrainer\stats (each run finds its stats file by scenario and time), ` +
        String.raw`FPSAimTrainer\Saved\SaveGames\Scenarios and workshop\content\824270 ` +
        "(each scenario's kind, time limit and target count). The browser remembers them.",
      run: then(() => this.folders.open()),
      files:
        state.refused || !this.folders.picker ? readStatsFiles(this.folders, this.local) : null,
    };
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
