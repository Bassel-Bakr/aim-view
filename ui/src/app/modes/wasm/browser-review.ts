import { Injectable, resource, ResourceRef } from '@angular/core';
import { Job, Report, Tracks } from '../../api';
import { ReviewEngine } from '../../platform/review-engine';

const NOT_YET = 'The review in the browser is not built yet.';

/**
 * The review in the browser: the detector (onnxruntime-web) and the review core (Rust, as WebAssembly), run in a
 * worker. Not built yet: every recording says so, and has no review.
 */
@Injectable({ providedIn: 'root' })
export class BrowserReview implements ReviewEngine {
  unavailable(): string | null {
    return NOT_YET;
  }

  report(id: () => string | undefined): ResourceRef<Report | null | undefined> {
    return resource({ params: id, loader: async () => null });
  }

  tracks(id: () => string | undefined): ResourceRef<Tracks | null | undefined> {
    return resource({ params: id, loader: async () => null });
  }

  async start(): Promise<Job> {
    return { stage: 'error', error: NOT_YET };
  }

  async job(): Promise<Job> {
    return { stage: 'none' };
  }
}
