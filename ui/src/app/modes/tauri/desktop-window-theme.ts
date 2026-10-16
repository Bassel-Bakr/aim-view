/**
 * The desktop window's title bar in the page's theme: Windows draws it in its own setting unless
 * the app names one. In: the viewer's theme choice (services/theme.ts). Out: POST
 * /api/window/theme to the app (desktop/src/protocol.rs), at start and after every change;
 * mode.desktop.ts starts it.
 */

import { HttpClient } from '@angular/common/http';
import { effect, inject, Service } from '@angular/core';
import { Theme } from '../../services/theme';

/** Tells the app's window the theme choice: light, dark, or system (Windows' own). */
@Service()
export class DesktopWindowTheme {
  /** Sends the choice to the app. */
  private readonly http = inject(HttpClient);
  /** The viewer's choice. */
  private readonly theme = inject(Theme);

  /** Sends the choice now and after each change. */
  constructor() {
    effect(() => this.send(this.theme.choice()));
  }

  /** Sends one choice; a failure leaves the title bar as it was. */
  private send(theme: string): void {
    this.http.post('/api/window/theme', null, { params: { theme } }).subscribe({
      error: () => undefined,
    });
  }
}
