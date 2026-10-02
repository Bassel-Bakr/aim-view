import { computed, inject, Injectable, resource, ResourceRef } from '@angular/core';
import { Job, StatsChange, StatsChoice, StatsHow, StatsPairing } from '../../api';
import { StatsFiles } from '../../platform/stats-files';
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

/** KovaaK's folders chosen as files, then the runs' stats found. */
function readChosenFiles(
  folders: KovaakFolders,
  local: LocalFiles,
): (files: File[]) => Promise<void> {
  return async (files) => {
    await folders.openFiles(files);
    await local.findAllStats();
  };
}

/**
 * The stats files of recordings opened in the browser: found in KovaaK's stats folder by name and time (once the user
 * chooses the folder), picked from its files by scenario, or a .csv chosen from this computer. All read in the
 * browser.
 */
@Injectable({ providedIn: 'root' })
export class LocalStatsFiles implements StatsFiles {
  private readonly local = inject(LocalFiles);
  private readonly folder = inject(StatsFolder);
  private readonly folders = inject(KovaakFolders);
  readonly searches = this.folder.ready;
  readonly chooseFolder = readChosenFiles(this.folders, this.local);

  readonly missing = computed<string | null>(() => {
    const state = this.folders.state();
    if (state.busy) return null;
    const missing = (['stats', 'scenarios', 'workshop'] as const).filter(
      (r) => !state.found.includes(r),
    );
    if (!missing.length) return null;
    return (
      `Missing: ${missing.map((r) => ROLE_NAMES[r]).join(', ')}. Give them with Stats folder at the top: ` +
      String.raw`FPSAimTrainer (in steamapps\common) for the stats and your scenarios, workshop\content\824270 ` +
      "for the workshop's. The stats folder lets each run find its stats file by scenario and time; the scenario " +
      "folders give each scenario's kind, time limit and target count. The browser keeps a copy of the stats " +
      'files: choose the folder again after new runs.'
    );
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
