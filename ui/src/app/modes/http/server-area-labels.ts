/**
 * Server mode's `AreaLabels`, which the desktop app uses too and browser mode extends. In: the
 * review service's /api/exclude, /api/find_areas and /api/area_kinds. Out: each recording's
 * excluded areas, the kinds and the finder's proposals, for the run page's Excluded areas editor.
 */

import { HttpClient, httpResource, HttpResourceRef } from '@angular/common/http';
import { inject, Service } from '@angular/core';
import { firstValueFrom } from 'rxjs';
import {
  AreaBox,
  AreaKind,
  AreaSet,
  FoundAreas,
  KeptAreas,
  KindEdit,
  RecordingAreas,
} from '../../api';
import { AreaLabels } from '../../platform/area-labels';
import { withKindIds } from '../web-files/area-kinds';

/**
 * The review service's areas (service/src/areas.rs): each recording's exclude.json, the kinds
 * (area_kinds.json), and the area finder (service/src/finder.rs, the core's src/areas.rs), which
 * learns from the areas saved there (area_examples.jsonl).
 */
@Service()
export class ServerAreaLabels implements AreaLabels {
  /** Sends the layout, find and save requests. */
  private readonly http = inject(HttpClient);
  /** The service always has the area finder. */
  readonly finderMissing = null;

  /** The recording's areas and every kind, as the service gives them (GET /api/exclude). */
  areas(id: () => string | undefined): HttpResourceRef<RecordingAreas | undefined> {
    return httpResource<RecordingAreas>(() => {
      const at = id();
      return at === undefined ? undefined : { url: '/api/exclude', params: { id: at } };
    });
  }

  /** The server gives KovOBS's layout with its kinds' names: they become ids here. */
  async layout(): Promise<AreaSet> {
    const got = await firstValueFrom(
      this.http.get<RecordingAreas>('/api/exclude', { params: { layout: 'kovobs' } }),
    );
    return { boxes: withKindIds(got.boxes, got.kinds), source: got.source };
  }

  /** The finder's proposal for the recording (GET /api/find_areas, with copy as 1 or 0). */
  find(id: string, copy: boolean): Promise<FoundAreas> {
    const params = { id, copy: copy ? '1' : '0' };
    return firstValueFrom(this.http.get<FoundAreas>('/api/find_areas', { params }));
  }

  /**
   * Keeps the areas (POST /api/exclude). The service also makes the review again when the shown one
   * was tracked with other areas, and gives that job.
   */
  save(id: string, boxes: AreaBox[]): Promise<KeptAreas> {
    return firstValueFrom(this.http.post<KeptAreas>('/api/exclude', boxes, { params: { id } }));
  }

  /** Adds or renames a kind (POST /api/area_kinds); gives every kind. */
  saveKind(kind: KindEdit): Promise<AreaKind[]> {
    return firstValueFrom(this.http.post<AreaKind[]>('/api/area_kinds', kind));
  }
}
