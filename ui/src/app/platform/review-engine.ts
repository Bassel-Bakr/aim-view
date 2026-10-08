/**
 * The ReviewEngine contract: starting, following and cancelling a recording's review, its report,
 * its tracks, its run window and the kind of run the user chose. In: each mode's implementation (modes/mode.*.ts). Out: the
 * Review service (services/review.ts), which the run page reads.
 */

import { ResourceRef } from '@angular/core';
import { Job, KindChange, Report, RunKind, RunMarks, Tracks } from '../api';

/**
 * What reviews the recordings: the review server, the review core in the browser, or the desktop app. Each mode
 * provides one (modes/mode.*.ts).
 */
export abstract class ReviewEngine {
  /** Why it cannot review this recording, in words; null when it can. */
  abstract unavailable(id: string): string | null;

  /** What the review of this recording will lack, in words (it can still run); null when nothing. */
  abstract caveat(id: string): string | null;

  /** The report of the recording's review, or null when it has none. Call it where a resource can be made. */
  abstract report(id: () => string | undefined): ResourceRef<Report | null | undefined>;

  /** Every target in every frame of the recording's review. Call it where a resource can be made. */
  abstract tracks(id: () => string | undefined): ResourceRef<Tracks | null | undefined>;

  /**
   * Starts a review: with again, a new one by the chosen model; else the one on show (measured again when its report
   * is gone), or a new one when there is none.
   */
  abstract start(id: string, again: boolean): Promise<Job>;

  /** The recording's review job as it stands. */
  abstract job(id: string): Promise<Job>;

  /** Cancels the recording's review while it runs: it stops and keeps nothing (the review shown before stays). */
  abstract cancel(id: string): Promise<Job>;

  /** The user's run window for the recording; null when none is marked. Call it where a resource can be made. */
  abstract marks(id: () => string | undefined): ResourceRef<RunMarks | null | undefined>;

  /**
   * Keeps the run window (null: none) and measures the review again with it. Where the review tracks only the window
   * (the browser, the desktop app), a review that did not track all of the new one is made again. Resolves to the job.
   */
  abstract setMarks(id: string, marks: RunMarks | null): Promise<Job>;

  /**
   * Keeps the kind of run the user chose for the recording (null: its scenario's again), which the report reads when
   * it is next shown. Resolves to the recording's kind now.
   */
  abstract setKind(id: string, kind: RunKind | null): Promise<KindChange>;
}
