/**
 * The recordings and the open one (`Library`). In: the RecordingSource contract and the URL's
 * ?id=. Out: every feature that needs the list or the open recording.
 */

import { computed, effect, inject, Service, signal } from '@angular/core';
import { Recording } from '../api';
import { RecordingSource } from '../platform/recording-source';
import { queryValue, setQuery } from './url-query';

/**
 * The recordings, from wherever this mode keeps them (RecordingSource), and the one that is open. The open one is kept
 * in the URL (?id=) when its id lasts, so a link opens it.
 */
@Service()
export class Library {
  /** Where this mode's recordings come from. */
  readonly source = inject(RecordingSource);
  /** Every recording, newest first. */
  readonly all = this.source.recordings;
  /** The open recording's id; null when none is open. */
  readonly selectedId = signal<string | null>(queryValue('id'));
  /** The open recording's row; null when none is open or it is not in the list (yet). */
  readonly selected = computed<Recording | null>(() => {
    const id = this.selectedId();
    return this.all().find((recording) => recording.id === id) ?? null;
  });

  /** Keeps the open recording's id in the URL while its id lasts across page loads. */
  constructor() {
    effect(() => {
      const id = this.selectedId();
      const shared = id !== null && this.source.lasting(id);
      setQuery({ id: shared ? id : null });
    });
  }
}
