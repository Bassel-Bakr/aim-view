/**
 * The `ScoreHistory` of every mode (in browser mode the service in the page answers it). In: the
 * review service's /api/history. Out: a scenario's past runs, for the run page's score history.
 */

import { httpResource, HttpResourceRef } from '@angular/common/http';
import { Service } from '@angular/core';
import { PastRun, ScoreHistory } from '../../platform/score-history';

/** A scenario's past runs, read by the review server from KovaaK's stats folder (/api/history). */
@Service()
export class ServerScoreHistory implements ScoreHistory {
  /** The scenario's past runs (GET /api/history); nothing is asked until the scenario is known. */
  runs(scenario: () => string | undefined): HttpResourceRef<PastRun[] | undefined> {
    return httpResource<PastRun[]>(() => {
      const name = scenario();
      return name === undefined ? undefined : { url: '/api/history', params: { scenario: name } };
    });
  }
}
