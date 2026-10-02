import { HttpClient, httpResource, HttpResourceRef } from '@angular/common/http';
import { inject, Injectable } from '@angular/core';
import { firstValueFrom } from 'rxjs';
import { Job, Report, Tracks } from '../../api';
import { ReviewEngine } from '../../platform/review-engine';

/** The review server reviews its recordings (python/review.py), with the model picked there. */
@Injectable({ providedIn: 'root' })
export class ServerReview implements ReviewEngine {
  private readonly http = inject(HttpClient);

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
}
