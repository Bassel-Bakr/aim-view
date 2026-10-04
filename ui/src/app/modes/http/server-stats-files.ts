import { HttpClient, httpResource, HttpResourceRef } from '@angular/common/http';
import { inject, Service, Signal, signal } from '@angular/core';
import { firstValueFrom } from 'rxjs';
import { StatsChange, StatsChoice, StatsPairing, Uploaded } from '../../api';
import { StatsFiles } from '../../platform/stats-files';
import { readStats } from '../web-files/stats-csv';

/**
 * The stats files of the review server's recordings: KovaaK's stats files it lists (/api/stats), the user's choice
 * (kept there), and a stats file sent from this computer (/api/upload).
 */
@Service()
export class ServerStatsFiles implements StatsFiles {
  private readonly http = inject(HttpClient);
  readonly searches: Signal<boolean> = signal(true).asReadonly();
  /** The server reaches the stats folder itself. */
  readonly missing: Signal<string | null> = signal<string | null>(null).asReadonly();
  readonly chooseFolder: ((files: File[]) => Promise<void>) | null = null;

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

  choose(id: string, choice: StatsChoice): Promise<StatsChange> {
    return firstValueFrom(this.http.post<StatsChange>('/api/stats', choice, { params: { id } }));
  }

  async pairFile(id: string, file: File): Promise<StatsChange> {
    if (!(await readStats(file)))
      throw new Error(`${file.name} is not one of KovaaK's stats files`);
    const sent = await firstValueFrom(
      this.http.post<Uploaded>('/api/upload', file, { params: { name: file.name, id } }),
    );
    return { job: sent.job ?? { stage: 'none' }, stats: sent.stats ?? true };
  }
}
