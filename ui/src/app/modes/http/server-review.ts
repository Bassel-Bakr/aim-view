import { HttpClient, httpResource, HttpResourceRef } from '@angular/common/http';
import { inject, Injectable } from '@angular/core';
import { firstValueFrom } from 'rxjs';
import { Job, Report, RunMarks, Tracks } from '../../api';
import { ReviewEngine } from '../../platform/review-engine';

/** A window with none of its three marks set is no window. */
function markedOrNull(m: RunMarks | null): RunMarks | null {
  return m && (m.start != null || m.end != null || m.length != null) ? m : null;
}

/** The review server reviews its recordings (python/review.py), with the model picked there. */
@Injectable({ providedIn: 'root' })
export class ServerReview implements ReviewEngine {
  private readonly http = inject(HttpClient);

  caveat(): string | null {
    return null;
  }

  unavailable(): string | null {
    return null;
  }

  report(id: () => string | undefined): HttpResourceRef<Report | null | undefined> {
    return httpResource<Report | null>(() => {
      const at = id();
      return at === undefined ? undefined : { url: '/api/report', params: { id: at } };
    });
  }

  tracks(id: () => string | undefined): HttpResourceRef<Tracks | null | undefined> {
    return httpResource<Tracks | null>(() => {
      const at = id();
      return at === undefined ? undefined : { url: '/api/tracks', params: { id: at } };
    });
  }

  start(id: string, again: boolean): Promise<Job> {
    const params: Record<string, string> = again ? { id, again: '1' } : { id };
    return firstValueFrom(this.http.post<Job>('/api/analyse', null, { params }));
  }

  job(id: string): Promise<Job> {
    return firstValueFrom(this.http.get<Job>('/api/job', { params: { id } }));
  }

  /** The window the server keeps (run.json): all three null when none is marked. */
  marks(id: () => string | undefined): HttpResourceRef<RunMarks | null | undefined> {
    return httpResource<RunMarks | null>(
      () => {
        const at = id();
        return at === undefined ? undefined : { url: '/api/run', params: { id: at } };
      },
      { parse: (v) => markedOrNull(v as RunMarks) },
    );
  }

  /** The server keeps the window and measures the run again on its tracks (it tracks the whole video). */
  setMarks(id: string, marks: RunMarks | null): Promise<Job> {
    const body: RunMarks = marks ?? { start: null, end: null, length: null };
    return firstValueFrom(this.http.post<Job>('/api/run', body, { params: { id } }));
  }
}
