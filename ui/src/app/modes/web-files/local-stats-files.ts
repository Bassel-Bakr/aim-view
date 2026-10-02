import { inject, Injectable, resource, ResourceRef } from '@angular/core';
import { Job, StatsChange, StatsChoice, StatsPairing } from '../../api';
import { StatsFiles } from '../../platform/stats-files';
import { LocalFiles, localRecording } from './local-files';
import { StatsCsv, statsSummary } from './stats-csv';

/** What a pairing is read from: the recording, and the stats file it has now. */
export interface LocalPairingParams {
  id: string;
  stats: StatsCsv | null;
}

const NO_JOB: Job = { stage: 'none' };

/**
 * The stats files of recordings opened from this computer: a .csv chosen from this computer, read in the browser. It
 * cannot list KovaaK's stats files yet: that needs the stats folder opened in the browser.
 */
@Injectable({ providedIn: 'root' })
export class LocalStatsFiles implements StatsFiles {
  private readonly local = inject(LocalFiles);
  readonly searches = false;

  pairing(id: () => string | undefined): ResourceRef<StatsPairing | undefined> {
    return resource({
      params: (): LocalPairingParams | undefined => {
        const at = id();
        const f = at === undefined ? null : this.local.find(at);
        return f ? { id: f.id, stats: f.stats } : undefined;
      },
      loader: async ({ params }) => {
        const f = this.local.find(params.id);
        return {
          file: params.stats?.name ?? null,
          how: params.stats ? 'upload' : 'missing',
          scenario: f ? localRecording(f).scenario : '',
          candidates: [],
          facts: params.stats ? statsSummary(params.stats) : undefined,
        };
      },
    });
  }

  /** None, or back to automatic (with no stats folder, that finds none either); a listed file cannot come up yet. */
  async choose(id: string, choice: StatsChoice): Promise<StatsChange> {
    if ('file' in choice && choice.file !== null)
      throw new Error("KovaaK's stats folder is not open in the browser yet");
    this.local.unpair(id);
    return { job: NO_JOB, stats: false };
  }

  async pairFile(id: string, file: File): Promise<StatsChange> {
    if (!(await this.local.pair(id, file)))
      throw new Error(`${file.name} is not one of KovaaK's stats files`);
    return { job: NO_JOB, stats: true };
  }
}
