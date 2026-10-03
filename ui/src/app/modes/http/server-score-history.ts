import { httpResource, HttpResourceRef } from '@angular/common/http';
import { Injectable } from '@angular/core';
import { PastRun, ScoreHistory } from '../../platform/score-history';

/** A scenario's past runs, read by the review server from KovaaK's stats folder (/api/history). */
@Injectable({ providedIn: 'root' })
export class ServerScoreHistory implements ScoreHistory {
  runs(scenario: () => string | undefined): HttpResourceRef<PastRun[] | undefined> {
    return httpResource<PastRun[]>(() => {
      const name = scenario();
      return name === undefined ? undefined : { url: '/api/history', params: { scenario: name } };
    });
  }
}
