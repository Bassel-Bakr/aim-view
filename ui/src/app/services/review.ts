import { computed, effect, inject, Injectable, signal } from '@angular/core';
import { errorMessage, Job } from '../api';
import { ReviewEngine } from '../platform/review-engine';
import { Library } from './library';

const POLL_MS = 500;

/**
 * The open recording's review, from this mode's ReviewEngine: its report and tracks, and the review job when one runs.
 * A finished job reloads the report (and with it the tracks), and marks the recording reviewed in the list.
 */
@Injectable({ providedIn: 'root' })
export class Review {
  private readonly engine = inject(ReviewEngine);
  private readonly library = inject(Library);
  readonly job = signal<Job>({ stage: 'none' });
  readonly running = computed(() => !['none', 'done', 'error'].includes(this.job().stage));
  /** Why the open recording cannot be reviewed here, in words; null when it can. */
  readonly unavailable = computed(() => {
    const id = this.library.selectedId();
    return id === null ? null : this.engine.unavailable(id);
  });
  private readonly reviewable = () => {
    const id = this.library.selectedId();
    return id !== null && this.engine.unavailable(id) === null ? id : undefined;
  };

  /** The report, or null when the recording has no review yet. */
  readonly report = this.engine.report(this.reviewable);

  /**
   * Every target in every frame, once there is a review: the tracking overlay and timeline draw them, and a clicking
   * run's fastest paths are worked out from them.
   */
  readonly tracks = this.engine.tracks(() =>
    this.report.hasValue() && this.report.value() ? this.reviewable() : undefined,
  );

  private watcher = 0;

  constructor() {
    effect(() => {
      const id = this.library.selectedId();
      this.job.set({ stage: 'none' });
      if (id !== null && this.engine.unavailable(id) === null) void this.watch(id);
    });
  }

  /** Starts a review of the open recording: a new one by the chosen model when again, else the cached one. */
  async analyse(again: boolean): Promise<void> {
    const id = this.library.selectedId();
    if (!id) return;
    this.watcher++;
    try {
      this.follow(await this.engine.start(id, again));
    } catch (e) {
      this.job.set({ stage: 'error', error: errorMessage(e) });
    }
  }

  /** Shows a job the server started for the open recording, and follows it until it ends. */
  follow(job: Job): void {
    const id = this.library.selectedId();
    this.job.set(job);
    if (id && !['none', 'error'].includes(job.stage)) void this.watch(id, true);
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
        const job = await this.engine.job(id);
        if (!current()) return;
        this.job.set(job);
        ran ||= this.running();
        if (job.stage === 'done' && ran) {
          this.library.source.patch(id, { analysed: true });
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
