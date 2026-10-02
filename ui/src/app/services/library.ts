import { httpResource } from '@angular/common/http';
import { computed, effect, inject, Injectable, signal } from '@angular/core';
import { Recording } from '../api';
import { isLocal, LocalFiles } from './local-files';

/**
 * The recordings (this browser's, then the review server's), and the one that is open. The open one is kept in the
 * URL (?id=), so a link opens it; a recording from this computer is not, since the link could not open it again.
 */
@Injectable({ providedIn: 'root' })
export class Library {
  private readonly local = inject(LocalFiles);
  /** The review server's recordings. */
  readonly recordings = httpResource<Recording[]>(() => '/api/vods');
  readonly all = computed<Recording[]>(() => [
    ...this.local.recordings(),
    ...(this.recordings.hasValue() ? this.recordings.value() : []),
  ]);
  readonly selectedId = signal<string | null>(new URLSearchParams(location.search).get('id'));
  readonly selected = computed<Recording | null>(() => {
    const id = this.selectedId();
    return this.all().find((r) => r.id === id) ?? null;
  });

  constructor() {
    effect(() => {
      const id = this.selectedId();
      const shared = id && !isLocal(id);
      history.replaceState(null, '', shared ? `?id=${encodeURIComponent(id)}` : location.pathname);
    });
  }
}
