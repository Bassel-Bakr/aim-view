/**
 * Server mode's `ReviewEngine`, which the desktop app uses too and browser mode extends. In: the
 * review service's /api/report, /api/tracks, /api/analyse, /api/job, /api/cancel, /api/run and
 * /api/kind.
 * Out: each recording's report, tracks, review job and run window, for the run page.
 */

import { HttpClient, httpResource, HttpResourceRef } from '@angular/common/http';
import { inject, Service } from '@angular/core';
import { firstValueFrom } from 'rxjs';
import { Job, KindChange, Report, RunKind, RunMarks, Tracks } from '../../api';
import { ReviewEngine } from '../../platform/review-engine';

/**
 * Whether a report fetched again is the one already shown, to the last number. It often is (a
 * switch to a model that has not reviewed the recording shows the same review), and the run page
 * then need not work out and draw it again.
 */
function sameReport(a: Report | null, b: Report | null): boolean {
  return a === b || (a !== null && b !== null && JSON.stringify(a) === JSON.stringify(b));
}

/** A window with none of its three marks set is no window. */
function markedOrNull(marks: RunMarks | null): RunMarks | null {
  return marks && (marks.start != null || marks.end != null || marks.length != null) ? marks : null;
}

/**
 * The job as the review sees it: a link's download is a job of the recording's too, but it is the
 * recording source's to follow, so it reads as no job ('none') here.
 */
function reviewOnly(job: Job): Job {
  return job.link ? { stage: 'none' } : job;
}

/**
 * The review service reviews its recordings natively (service/src/review.rs and the core), with
 * the model picked there.
 */
@Service()
export class ServerReview implements ReviewEngine {
  /** Sends the start, job, cancel and run window requests. */
  private readonly http = inject(HttpClient);

  /** Always null: the service's review lacks nothing. */
  caveat(): string | null {
    return null;
  }

  /** Always null: the service can review every recording it lists. */
  unavailable(): string | null {
    return null;
  }

  /** The recording's report (GET /api/report); a fetch equal to the shown one keeps it. */
  report(id: () => string | undefined): HttpResourceRef<Report | null | undefined> {
    return httpResource<Report | null>(
      () => {
        const at = id();
        return at === undefined ? undefined : { url: '/api/report', params: { id: at } };
      },
      { equal: sameReport },
    );
  }

  /** Every target in every frame of the shown review (GET /api/tracks). */
  tracks(id: () => string | undefined): HttpResourceRef<Tracks | null | undefined> {
    return httpResource<Tracks | null>(() => {
      const at = id();
      return at === undefined ? undefined : { url: '/api/tracks', params: { id: at } };
    });
  }

  /** Starts the review on the service (POST /api/analyse, with again=1 for a new one). */
  start(id: string, again: boolean): Promise<Job> {
    const params: Record<string, string> = again ? { id, again: '1' } : { id };
    return firstValueFrom(this.http.post<Job>('/api/analyse', null, { params })).then(reviewOnly);
  }

  /** The recording's review job (GET /api/job), a link's download read as none. */
  job(id: string): Promise<Job> {
    return firstValueFrom(this.http.get<Job>('/api/job', { params: { id } })).then(reviewOnly);
  }

  /** Cancels the recording's job on the service (POST /api/cancel). */
  cancel(id: string): Promise<Job> {
    return firstValueFrom(this.http.post<Job>('/api/cancel', null, { params: { id } })).then(
      reviewOnly,
    );
  }

  /**
   * The run window the service keeps (run.json, GET /api/run); null when none is marked (the
   * service answers all three marks null then).
   */
  marks(id: () => string | undefined): HttpResourceRef<RunMarks | null | undefined> {
    return httpResource<RunMarks | null>(
      () => {
        const at = id();
        return at === undefined ? undefined : { url: '/api/run', params: { id: at } };
      },
      { parse: (value) => markedOrNull(value as RunMarks) },
    );
  }

  /**
   * Keeps the window (POST /api/run; all three marks null for none). The service measures the
   * review again, or makes it again when it did not track all of the new window, and gives that
   * job.
   */
  setMarks(id: string, marks: RunMarks | null): Promise<Job> {
    const body: RunMarks = marks ?? { start: null, end: null, length: null };
    return firstValueFrom(this.http.post<Job>('/api/run', body, { params: { id } }));
  }

  /** Keeps the kind (POST /api/kind; null follows the scenario again) and gives the row's kind now. */
  setKind(id: string, kind: RunKind | null): Promise<KindChange> {
    return firstValueFrom(this.http.post<KindChange>('/api/kind', { kind }, { params: { id } }));
  }
}
