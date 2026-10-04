import { httpResource, HttpResourceRef } from '@angular/common/http';
import { inject, Injectable } from '@angular/core';
import { AreaBox, AreaKind, FoundAreas, KeptAreas, KindEdit, RecordingAreas } from '../../api';
import { ServerAreaLabels } from '../http/server-area-labels';
import { BrowserExamples } from './browser-labelling';
import { BrowserReview } from './browser-review';
import { needsFound, PageAreaFinder } from './page-area-finder';

/**
 * Browser mode's areas: the review service keeps them and learns from them, as the review server does. Where the
 * service has no found areas for a recording yet (its 409), the page's area finder reads the video first; a review the
 * service starts for areas changed is run by the page (BrowserReview).
 */
@Injectable({ providedIn: 'root' })
export class BrowserAreaLabels extends ServerAreaLabels {
  private readonly finder = inject(PageAreaFinder);
  private readonly review = inject(BrowserReview);
  private readonly examples = inject(BrowserExamples);

  /** Read again when the kinds change here (a kinds file loaded with the area finder's examples). */
  override areas(id: () => string | undefined): HttpResourceRef<RecordingAreas | undefined> {
    return httpResource<RecordingAreas>(() => {
      this.examples.changes();
      const at = id();
      return at === undefined ? undefined : { url: '/api/exclude', params: { id: at } };
    });
  }

  /** The service's proposal; when it needs the area finder's reading of the video, read first, then asked again. */
  override async find(id: string, copy: boolean): Promise<FoundAreas> {
    try {
      return await super.find(id, copy);
    } catch (e) {
      const need = needsFound(e);
      if (!need) throw e;
      await this.finder.find(id, need.video);
      return super.find(id, copy);
    }
  }

  /** Kept by the service, which learns from them (the examples change); a review it starts, the page runs. */
  override async save(id: string, boxes: AreaBox[]): Promise<KeptAreas> {
    const kept = await super.save(id, boxes);
    void this.examples.changed();
    return kept.job ? { ...kept, job: this.review.follow(id, kept.job) } : kept;
  }

  override async saveKind(kind: KindEdit): Promise<AreaKind[]> {
    const kinds = await super.saveKind(kind);
    void this.examples.changed();
    return kinds;
  }
}
