import { computed, effect, inject, Injectable, signal } from '@angular/core';
import { Recording } from '../api';
import { RecordingSource } from '../platform/recording-source';

/**
 * The recordings, from wherever this mode keeps them (RecordingSource), and the one that is open. The open one is kept
 * in the URL (?id=) when its id lasts, so a link opens it.
 */
@Injectable({ providedIn: 'root' })
export class Library {
  readonly source = inject(RecordingSource);
  readonly all = this.source.recordings;
  readonly selectedId = signal<string | null>(new URLSearchParams(location.search).get('id'));
  readonly selected = computed<Recording | null>(() => {
    const id = this.selectedId();
    return this.all().find((r) => r.id === id) ?? null;
  });

  constructor() {
    effect(() => {
      const id = this.selectedId();
      const shared = id !== null && this.source.lasting(id);
      history.replaceState(null, '', shared ? `?id=${encodeURIComponent(id)}` : location.pathname);
    });
  }
}
