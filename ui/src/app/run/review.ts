import { HttpClient, httpResource } from '@angular/common/http';
import { computed, effect, inject, Injectable, signal } from '@angular/core';
import { firstValueFrom } from 'rxjs';
import { errorMessage, Job, Report, Tracks } from '../api';
import { Library } from '../services/library';

const POLL_MS = 500;

/**
 * The open recording's review: its report and tracks, and the review job when one runs. A finished job reloads the
 * report (and with it the tracks), and marks the recording reviewed in the list.
 */
@Injectable({ providedIn: 'root' })
export class Review {
  private readonly http = inject(HttpClient);
  private readonly library = inject(Library);
  readonly job = signal<Job>({ stage: 'none' });
  readonly running = computed(() => !['none', 'done', 'error'].includes(this.job().stage));

  /** The report, or null when the recording has no review yet. */
  readonly report = httpResource<Report | null>(() => {
    const id = this.library.selectedId();
    return id ? { url: '/api/report', params: { id } } : undefined;
  });

  /** Every target in every frame, which the tracking overlay and timeline draw. Clicking runs do not need them. */
  readonly tracks = httpResource<Tracks | null>(() => {
    const id = this.library.selectedId();
    const report = this.report.hasValue() ? this.report.value() : null;
    return id && report?.mode === 'track' ? { url: '/api/tracks', params: { id } } : undefined;
  });

  private watcher = 0;

  constructor() {
    effect(() => {
      const id = this.library.selectedId();
      this.job.set({ stage: 'none' });
      if (id) void this.watch(id);
    });
  }

  /** Starts a review of the open recording: a new one by the chosen model when again, else the cached one. */
  async analyse(again: boolean): Promise<void> {
    const id = this.library.selectedId();
    if (!id) return;
    this.watcher++;
    const params: Record<string, string> = again ? { id, again: '1' } : { id };
    try {
      this.job.set(await firstValueFrom(this.http.post<Job>('/api/analyse', null, { params })));
    } catch (e) {
      this.job.set({ stage: 'error', error: errorMessage(e) });
      return;
    }
    if (this.job().stage !== 'error') void this.watch(id, true);
  }

  /**
   * Follows the recording's job until it ends. A new watcher replaces the one before. Only a job seen running (or just
   * started) reloads the review when it is done: a job that ended before the recording was opened changes nothing.
   */
  private async watch(id: string, started = false): Promise<void> {
    const me = ++this.watcher;
    const current = () => this.watcher === me && this.library.selectedId() === id;
    let ran = started;
    try {
      while (current()) {
        const job = await firstValueFrom(this.http.get<Job>('/api/job', { params: { id } }));
        if (!current()) return;
        this.job.set(job);
        ran ||= this.running();
        if (job.stage === 'done' && ran) {
          this.library.recordings.update((list) =>
            list?.map((r) => (r.id === id ? { ...r, analysed: true } : r)),
          );
          this.report.reload();
          return;
        }
        if (!this.running()) return;
        await new Promise((r) => setTimeout(r, POLL_MS));
      }
    } catch (e) {
      if (current()) this.job.set({ stage: 'error', error: errorMessage(e) });
    }
  }
}
