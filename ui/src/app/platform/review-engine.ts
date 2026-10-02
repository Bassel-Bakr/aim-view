import { ResourceRef } from '@angular/core';
import { Job, Report, Tracks } from '../api';

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
}
