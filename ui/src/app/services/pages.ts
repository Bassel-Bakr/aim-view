/**
 * Which page shows and whether the recordings list is open (`Pages`). In: the URL's ?page=, the
 * open recording, the screen's width and what this browser kept. Out: the root component's layout.
 */

import { effect, inject, Service, signal, untracked } from '@angular/core';
import { Library } from './library';
import { queryValue, setQuery } from './url-query';

/** Where the recordings list's open or closed state is kept on a wide screen. */
const LIST_KEY = 'aimview-list';

/** The page's width at which the app takes its narrow layout (themes/layout.scss --narrow-width); null without one. */
function narrowQuery(): MediaQueryList | null {
  if (typeof matchMedia !== 'function') return null;
  const width = getComputedStyle(document.documentElement)
    .getPropertyValue('--narrow-width')
    .trim();
  return width ? matchMedia(`(width <= ${width})`) : null;
}

/** Whether this browser kept the list's column open (open when it kept nothing). */
function storedOpen(): boolean {
  try {
    return localStorage.getItem(LIST_KEY) !== 'closed';
  } catch {
    return true;
  }
}

/**
 * Which page the app shows (the recordings and the run page, or the Crops page, kept in the URL as ?page=crops so a
 * link opens it) and whether the recordings list is open. On a wide screen the list is a column the user opens and
 * closes (kept across visits); on a narrow one (a phone) it is a drawer over the page, open while no recording is,
 * and closing when one is picked.
 */
@Service()
export class Pages {
  /** The open recording, which closes the drawer when one is picked. */
  private readonly library = inject(Library);
  /** Whether the Crops page shows, not the review page. */
  readonly crops = signal(queryValue('page') === 'crops');
  /** Whether the screen is narrow (a phone): the list is a drawer. */
  readonly narrow = signal(false);
  /** Whether the list's column is open on a wide screen. */
  private readonly columnOpen = signal(storedOpen());
  /** Whether the list's drawer is open on a narrow screen. */
  private readonly drawerOpen = signal(true);

  /** Keeps ?page= in the URL, opens the drawer while no recording is open, follows the width. */
  constructor() {
    effect(() => setQuery({ page: this.crops() ? 'crops' : null }));
    effect(() => {
      const id = this.library.selectedId();
      untracked(() => this.drawerOpen.set(id === null));
    });
    const query = narrowQuery();
    if (query) {
      this.narrow.set(query.matches);
      query.addEventListener('change', (event) => this.narrow.set(event.matches));
    }
  }

  /** Whether the recordings list shows: the drawer on a narrow screen, the column on a wide one. */
  listOpen(): boolean {
    return this.narrow() ? this.drawerOpen() : this.columnOpen();
  }

  /** Switches between the Crops page and the review page. */
  toggleCrops(): void {
    this.crops.update((open) => !open);
  }

  /** Opens or closes the recordings list; a wide screen's choice is kept across visits. */
  toggleList(): void {
    if (this.narrow()) {
      this.drawerOpen.update((open) => !open);
      return;
    }
    const open = !this.columnOpen();
    this.columnOpen.set(open);
    try {
      localStorage.setItem(LIST_KEY, open ? 'open' : 'closed');
    } catch {
      // the browser keeps nothing: the list opens again next visit
    }
  }
}
