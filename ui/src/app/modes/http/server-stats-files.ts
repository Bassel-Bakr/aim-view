/**
 * Server mode's `StatsFiles`, which the desktop app uses too and browser mode extends. In: the
 * review service's /api/stats and /api/upload, and a stats file the user picks. Out: each
 * recording's stats file and the files to pair it with, for the run page's stats file panel.
 */

import { HttpClient, httpResource, HttpResourceRef } from '@angular/common/http';
import { inject, Service, Signal, signal } from '@angular/core';
import { firstValueFrom } from 'rxjs';
import { StatsChange, StatsChoice, StatsPairing, Uploaded } from '../../api';
import { StatsFiles } from '../../platform/stats-files';
import { readStats } from '../web-files/stats-csv';

/**
 * The stats files of the review server's recordings: KovaaK's stats files it lists (/api/stats),
 * the user's choice (kept there), and a stats file sent from this computer (/api/upload).
 */
@Service()
export class ServerStatsFiles implements StatsFiles {
  /** Sends the choice and the uploads. */
  private readonly http = inject(HttpClient);
  /** Always true: the server reaches KovaaK's stats folder, so it can list files to pair with. */
  readonly searches: Signal<boolean> = signal(true).asReadonly();
  /** The server reaches the stats folder itself. */
  readonly missing: Signal<string | null> = signal<string | null>(null).asReadonly();
  /** The server reads the stats folder where it is, so the page chooses none. */
  readonly chooseFolder: ((files: File[]) => Promise<void>) | null = null;

  /** The recording's stats file and the files to pair it with (GET /api/stats, q the query). */
  pairing(
    id: () => string | undefined,
    query: () => string | null,
  ): HttpResourceRef<StatsPairing | undefined> {
    return httpResource<StatsPairing>(() => {
      const at = id();
      if (at === undefined) return undefined;
      const search = query();
      const params: Record<string, string> = { id: at };
      if (search !== null) params['q'] = search;
      return { url: '/api/stats', params };
    });
  }

  /**
   * Sends the user's choice (POST /api/stats); gives the job measuring the review again and
   * whether the recording has a stats file now.
   */
  choose(id: string, choice: StatsChoice): Promise<StatsChange> {
    return firstValueFrom(this.http.post<StatsChange>('/api/stats', choice, { params: { id } }));
  }

  /**
   * Checks the file is one of KovaaK's stats files, then sends it with the recording's id
   * (POST /api/upload). Rejects any other file before sending it.
   */
  async pairFile(id: string, file: File): Promise<StatsChange> {
    if (!(await readStats(file)))
      throw new Error(`${file.name} is not one of KovaaK's stats files`);
    const sent = await firstValueFrom(
      this.http.post<Uploaded>('/api/upload', file, { params: { name: file.name, id } }),
    );
    return { job: sent.job ?? { stage: 'none' }, stats: sent.stats ?? true };
  }
}
