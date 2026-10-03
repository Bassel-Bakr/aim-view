import { HttpClient, httpResource, HttpResourceRef } from '@angular/common/http';
import { inject, Injectable } from '@angular/core';
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
 * The review server's areas (python/server.py): each recording's exclude.json, the kinds (area_kinds.json), and the
 * area finder (python/areas.py), which learns from the areas saved there (area_examples.jsonl).
 */
@Injectable({ providedIn: 'root' })
export class ServerAreaLabels implements AreaLabels {
  private readonly http = inject(HttpClient);
  readonly finderMissing = null;

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

  find(id: string, copy: boolean): Promise<FoundAreas> {
    const params = { id, copy: copy ? '1' : '0' };
    return firstValueFrom(this.http.get<FoundAreas>('/api/find_areas', { params }));
  }

  /** The desktop app also starts a new review when the shown one was tracked with other areas (job). */
  save(id: string, boxes: AreaBox[]): Promise<KeptAreas> {
    return firstValueFrom(this.http.post<KeptAreas>('/api/exclude', boxes, { params: { id } }));
  }

  saveKind(kind: KindEdit): Promise<AreaKind[]> {
    return firstValueFrom(this.http.post<AreaKind[]>('/api/area_kinds', kind));
  }
}
