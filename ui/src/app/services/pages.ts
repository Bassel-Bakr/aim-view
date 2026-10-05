import { effect, Service, signal } from '@angular/core';
import { queryValue, setQuery } from './url-query';

/**
 * Which page the app shows: the recordings and the run page, or the Crops page (checking detector crops). The Crops
 * page is kept in the URL (?page=crops), so a link (the phone's bookmark) opens it.
 */
@Service()
export class Pages {
  readonly crops = signal(queryValue('page') === 'crops');

  constructor() {
    effect(() => setQuery({ page: this.crops() ? 'crops' : null }));
  }

  toggleCrops(): void {
    this.crops.update((open) => !open);
  }
}
