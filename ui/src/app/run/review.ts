import { computed, effect, inject, Injectable, resource, signal } from '@angular/core';
import { getJson, Job, postJson, Report, Tracks } from '../api';
import { Library } from '../services/library';

const POLL_MS = 500;

/**
 * The open recording's review: its report and tracks, and the review job when one runs. A finished job reloads both,
 * and marks the recording reviewed in the list.
 */
@Injectable({ providedIn: 'root' })
export class Review {
  private readonly library = inject(Library);
  private readonly version = signal(0);
  readonly job = signal<Job>({ stage: 'none' });
  readonly running = computed(() => !['none', 'done', 'error'].includes(this.job().stage));

  readonly report = resource({
    params: () => ({ id: this.library.selectedId(), version: this.version() }),
    loader: ({ params, abortSignal }) =>
      params.id
        ? getJson<Report | null>(`/api/report?id=${encodeURIComponent(params.id)}`, abortSignal)
        : Promise.resolve(null),
  });

  /** Every target in every frame, which the tracking overlay and timeline draw. Clicking runs do not need them. */
  readonly tracks = resource({
    params: () => {
      const id = this.library.selectedId();
      const report = this.report.hasValue() ? this.report.value() : null;
      return id && report?.mode === 'track' ? { id, version: this.version() } : undefined;
    },
    loader: ({ params, abortSignal }) =>
      getJson<Tracks | null>(`/api/tracks?id=${encodeURIComponent(params.id)}`, abortSignal),
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
    try {
      this.job.set(
        await postJson<Job>(`/api/analyse?id=${encodeURIComponent(id)}${again ? '&again=1' : ''}`),
      );
    } catch (e) {
      this.job.set({ stage: 'error', error: (e as Error).message });
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
        const job = await getJson<Job>(`/api/job?id=${encodeURIComponent(id)}`);
        if (!current()) return;
        this.job.set(job);
        ran ||= this.running();
        if (job.stage === 'done' && ran) {
          this.library.recordings.update((list) =>
            list?.map((r) => (r.id === id ? { ...r, analysed: true } : r)),
          );
          this.version.update((v) => v + 1);
          return;
        }
        if (!this.running()) return;
        await new Promise((r) => setTimeout(r, POLL_MS));
      }
    } catch (e) {
      if (current()) this.job.set({ stage: 'error', error: (e as Error).message });
    }
  }
}
