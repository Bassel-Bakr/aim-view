import { httpResource } from '@angular/common/http';
import { computed, Injectable } from '@angular/core';
import { ModelList } from '../api';

/** The detector models, and the one new reviews use (the top bar and the run page show it). */
@Injectable({ providedIn: 'root' })
export class Models {
  readonly list = httpResource<ModelList>(() => '/api/models');
  /** The model new reviews use, or null until the list loads. */
  readonly chosen = computed<string | null>(() =>
    this.list.hasValue() ? this.list.value().chosen : null,
  );
}

/** A model's name as people read it: "hand" is the hand-written detector. */
export function modelName(name: string): string {
  return name === 'hand' ? 'hand-written' : name;
}
