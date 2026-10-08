/**
 * The page's color theme: the viewer's choice (System, Light or Dark), kept in this browser's storage, and the scheme
 * in force (light or dark: the choice, or the system's under System). The choice is the root's data-theme, which
 * styles.scss puts the light tokens under (and index.html sets before the first paint); System removes it, so the
 * system's prefers-color-scheme decides. canvasStyle() gives a canvas the colors and fonts it reads from the tokens,
 * read again after a change, so a chart never deals with the theme itself. In: the top bar's theme menu.
 * Out: data-theme on the page's root, the scheme, and canvasStyle().
 */

import { computed, DestroyRef, DOCUMENT, inject, Service, Signal, signal } from '@angular/core';

/** The viewer's choice: follow the system, or one scheme always. */
export type ThemeChoice = 'system' | 'light' | 'dark';
/** A scheme in force. */
export type ColorScheme = 'light' | 'dark';

/** The storage key the choice is kept under (index.html reads it too, before the app starts). */
export const THEME_STORAGE_KEY = 'aimview.theme';
/** The system's own choice, as a media query. */
const SYSTEM_LIGHT = '(prefers-color-scheme: light)';

/** The theme the page shows, and the viewer's choice of it. */
@Service()
export class Theme {
  /** The page whose root carries the choice. */
  private readonly document = inject(DOCUMENT);
  /** The system's scheme, followed while it changes; null where the page cannot ask (a test's page). */
  private readonly systemQuery = this.document.defaultView?.matchMedia?.(SYSTEM_LIGHT) ?? null;
  /** The system's scheme now. */
  private readonly system = signal<ColorScheme>(this.systemQuery?.matches ? 'light' : 'dark');

  /** The viewer's choice. */
  readonly choice = signal<ThemeChoice>(this.storedChoice());
  /** The scheme in force: the choice, or the system's under System. */
  readonly scheme = computed<ColorScheme>(() => {
    const choice = this.choice();
    return choice === 'system' ? this.system() : choice;
  });

  /** Puts the stored choice on the page and follows the system's scheme. */
  constructor() {
    this.apply(this.choice());
    const query = this.systemQuery;
    if (!query) return;
    const follow = (): void => this.system.set(query.matches ? 'light' : 'dark');
    query.addEventListener('change', follow);
    inject(DestroyRef).onDestroy(() => query.removeEventListener('change', follow));
  }

  /** Makes a choice: shown at once, and kept for the next visit. */
  choose(choice: ThemeChoice): void {
    this.choice.set(choice);
    this.apply(choice);
    try {
      if (choice === 'system') localStorage.removeItem(THEME_STORAGE_KEY);
      else localStorage.setItem(THEME_STORAGE_KEY, choice);
    } catch {
      // storage refused (a private window): the choice holds for this visit
    }
  }

  /** The choice as the root's data-theme: none under System. */
  private apply(choice: ThemeChoice): void {
    const root = this.document.documentElement;
    if (choice === 'system') delete root.dataset['theme'];
    else root.dataset['theme'] = choice;
  }

  /** The choice kept in this browser, or System. */
  private storedChoice(): ThemeChoice {
    try {
      const stored = localStorage.getItem(THEME_STORAGE_KEY);
      return stored === 'light' || stored === 'dark' ? stored : 'system';
    } catch {
      return 'system';
    }
  }
}

/**
 * A canvas's colors and fonts: what `read` takes from the tokens in force on the canvas, read again after a theme
 * change. Make it a field (it needs an injection context) and read it in the canvas's draw effect, which then redraws
 * after a change too.
 */
export function canvasStyle<E extends Element, T>(
  canvas: () => E,
  read: (element: E) => T,
): Signal<T> {
  const theme = inject(Theme);
  return computed(() => {
    theme.scheme();
    return read(canvas());
  });
}
