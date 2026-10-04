import { inject, Injectable, resource, ResourceRef } from '@angular/core';
import { PastRun, ScoreHistory } from '../../platform/score-history';
import { pastRun, readFooter } from './stats-footer';
import { StatsFolder } from './stats-folder';

/** Stats files read at once. */
const READ_AT_ONCE = 64;

/** What the runs are read from: the scenario, and whether the stats folder is listed. */
export interface HistoryParams {
  scenario: string;
  ready: boolean;
}

/**
 * A scenario's past runs, read in the browser from the stats folder the user chose (or the copy kept from a visit
 * before, StatsCache): the end of each of its stats files, read once and kept by name for this visit.
 */
@Injectable({ providedIn: 'root' })
export class LocalScoreHistory implements ScoreHistory {
  private readonly folder = inject(StatsFolder);
  /** The runs read so far, by file name (null: the file has no score). */
  private readonly read = new Map<string, PastRun | null>();

  runs(scenario: () => string | undefined): ResourceRef<PastRun[] | undefined> {
    return resource({
      params: (): HistoryParams | undefined => {
        const name = scenario();
        return name === undefined ? undefined : { scenario: name, ready: this.folder.ready() };
      },
      loader: ({ params }) => (params.ready ? this.history(params.scenario) : Promise.resolve([])),
    });
  }

  private async history(scenario: string): Promise<PastRun[]> {
    const files = [...this.folder.entries(scenario)].sort(
      (a, b) => a.seconds - b.seconds || (a.name < b.name ? -1 : a.name > b.name ? 1 : 0),
    );
    const unread = files.filter((f) => !this.read.has(f.name));
    for (let i = 0; i < unread.length; i += READ_AT_ONCE) {
      await Promise.all(
        unread.slice(i, i + READ_AT_ONCE).map(async (f) => {
          const meta = await readFooter(await this.folder.read(f.name)).catch(() => null);
          this.read.set(f.name, meta && pastRun(f.stamp, meta));
        }),
      );
    }
    return files.map((f) => this.read.get(f.name)).filter((r): r is PastRun => !!r);
  }
}
