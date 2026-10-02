import { computed, effect, Injectable, resource, signal } from '@angular/core';
import { getJson, Recording } from '../api';

/** The recordings, and the one that is open. The open one is kept in the URL (?id=), so a link opens it. */
@Injectable({ providedIn: 'root' })
export class Library {
  readonly recordings = resource({
    loader: ({ abortSignal }) => getJson<Recording[]>('/api/vods', abortSignal),
  });
  readonly selectedId = signal<string | null>(new URLSearchParams(location.search).get('id'));
  readonly selected = computed<Recording | null>(() => {
    const id = this.selectedId();
    return this.recordings.hasValue()
      ? (this.recordings.value().find((r) => r.id === id) ?? null)
      : null;
  });

  constructor() {
    effect(() => {
      const id = this.selectedId();
      history.replaceState(null, '', id ? `?id=${encodeURIComponent(id)}` : location.pathname);
    });
  }
}
