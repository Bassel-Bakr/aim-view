/**
 * Browser mode's `AreaLabels`. In: the service in the page's area routes, and the page's area
 * finder (page-area-finder.ts) where the service needs the video read. Out: each recording's areas
 * and the finder's proposals for the Excluded areas editor; a review that saved areas start is run
 * in the page.
 */

import { httpResource, HttpResourceRef } from '@angular/common/http';
import { inject, Service } from '@angular/core';
import { AreaBox, AreaKind, FoundAreas, KeptAreas, KindEdit, RecordingAreas } from '../../api';
import { ServerAreaLabels } from '../http/server-area-labels';
import { BrowserExamples } from './browser-labelling';
import { BrowserReview } from './browser-review';
import { needsFound, PageAreaFinder } from './page-area-finder';

/**
 * Browser mode's areas: the review service keeps them and learns from them, as the review server
 * does. Where the service has no found areas for a recording yet (its 409), the page's area finder
 * reads the video first; the page runs a review the service starts for areas changed
 * (BrowserReview).
 */
@Service()
export class BrowserAreaLabels extends ServerAreaLabels {
  /** Reads a recording's video for the service when it has no found areas. */
  private readonly finder = inject(PageAreaFinder);
  /** Runs the review the service starts when the areas change. */
  private readonly review = inject(BrowserReview);
  /** Says when the examples or the kinds change, so the areas are read again. */
  private readonly examples = inject(BrowserExamples);

  /**
   * The recording's areas and kinds, read again when the kinds change here (a kinds file loaded
   * with the area finder's examples).
   */
  override areas(id: () => string | undefined): HttpResourceRef<RecordingAreas | undefined> {
    return httpResource<RecordingAreas>(() => {
      this.examples.changes();
      const at = id();
      return at === undefined ? undefined : { url: '/api/exclude', params: { id: at } };
    });
  }

  /**
   * The service's proposal; when it needs the area finder's reading of the video, the page reads it
   * first, then asks again.
   */
  override async find(id: string, copy: boolean): Promise<FoundAreas> {
    try {
      return await super.find(id, copy);
    } catch (error) {
      const need = needsFound(error);
      if (!need) throw error;
      await this.finder.find(id, need.video);
      return super.find(id, copy);
    }
  }

  /**
   * Kept by the service, which learns from them (the examples change); the page runs a review it
   * starts.
   */
  override async save(id: string, boxes: AreaBox[]): Promise<KeptAreas> {
    const kept = await super.save(id, boxes);
    void this.examples.changed();
    return kept.job ? { ...kept, job: this.review.follow(id, kept.job) } : kept;
  }

  /** Adds or renames a kind in the service, then says the kinds changed. */
  override async saveKind(kind: KindEdit): Promise<AreaKind[]> {
    const kinds = await super.saveKind(kind);
    void this.examples.changed();
    return kinds;
  }
}
