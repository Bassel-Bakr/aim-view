/**
 * The open recording's review (`Review`): its report, tracks, run window and job. In: the
 * ReviewEngine contract and the open recording (Library). Out: the run page, the faint cut-off and
 * the model panel, which read and start reviews through it.
 */

import { computed, effect, inject, Service, signal } from '@angular/core';
import { errorMessage, Job, RunKind, RunMarks } from '../api';
import { ReviewEngine } from '../platform/review-engine';
import { Library } from './library';

/** How often a running job is asked how it stands, in ms. */
const POLL_MS = 500;

/**
 * The open recording's review, from this mode's ReviewEngine: its report and tracks, and the review job when one runs.
 * A finished job reloads the report (and with it the tracks), and marks the recording reviewed in the list.
 */
@Service()
export class Review {
  /** The mode's reviews. */
  private readonly engine = inject(ReviewEngine);
  /** Which recording is open. */
  private readonly library = inject(Library);
  /** The open recording's job as last seen; none when there is none. */
  readonly job = signal<Job>({ stage: 'none' });
  /** Whether the job is still at work (not none, done, failed or cancelled). */
  readonly running = computed(
    () => !['none', 'done', 'error', 'cancelled'].includes(this.job().stage),
  );
  /** Why the open recording cannot be reviewed here, in words; null when it can. */
  readonly unavailable = computed(() => {
    const id = this.library.selectedId();
    return id === null ? null : this.engine.unavailable(id);
  });
  /** What the open recording's review will lack, in words; null when nothing. */
  readonly caveat = computed(() => {
    const id = this.library.selectedId();
    return id === null ? null : this.engine.caveat(id);
  });
  /** The open recording's id when this mode can review it; else undefined, so nothing loads. */
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

  /** The user's run window for the open recording; null when none is marked. */
  readonly marks = this.engine.marks(this.reviewable);

  /** The number of the newest watcher; an older watcher sees it changed and stops. */
  private watcher = 0;

  /** When another recording opens, its job is followed (a review may be running for it). */
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
    } catch (error) {
      this.job.set({ stage: 'error', error: errorMessage(error) });
    }
  }

  /** Cancels the open recording's review: it stops and keeps nothing, so the review shown before stays. */
  async cancel(): Promise<void> {
    const id = this.library.selectedId();
    if (!id) return;
    this.watcher++;
    try {
      this.job.set(await this.engine.cancel(id));
    } catch (error) {
      this.job.set({ stage: 'error', error: errorMessage(error) });
    }
  }

  /**
   * Keeps the kind of run the user chose for the open recording (null: its scenario's again), puts it in the list,
   * and reads the report again, which is worked out with it (a tracking or a clicking report).
   */
  async saveKind(kind: RunKind | null): Promise<void> {
    const id = this.library.selectedId();
    if (!id) return;
    try {
      const change = await this.engine.setKind(id, kind);
      this.library.source.patch(id, change);
      this.report.reload();
    } catch (error) {
      this.job.set({ stage: 'error', error: errorMessage(error) });
    }
  }

  /** Keeps the open recording's run window (null: none), and follows the review it measures again or makes again. */
  async saveMarks(marks: RunMarks | null): Promise<void> {
    const id = this.library.selectedId();
    if (!id) return;
    this.watcher++;
    try {
      const job = await this.engine.setMarks(id, marks);
      this.marks.reload();
      this.follow(job);
    } catch (error) {
      this.job.set({ stage: 'error', error: errorMessage(error) });
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
        await new Promise((resolve) => setTimeout(resolve, POLL_MS));
      }
    } catch (error) {
      if (current()) this.job.set({ stage: 'error', error: errorMessage(error) });
    }
  }
}
