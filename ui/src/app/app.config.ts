/**
 * The app's providers: HttpClient and the build's mode (modes/mode.ts, which the build swaps for
 * mode.browser.ts, mode.server.ts or mode.desktop.ts). Out: main.ts, which starts the app with
 * them.
 */

import { provideHttpClient, withInterceptors } from '@angular/common/http';
import { ApplicationConfig, provideBrowserGlobalErrorListeners } from '@angular/core';
import { MODE } from './modes/mode';

/** The providers the app starts with: error listeners, HttpClient, the mode's services. */
export const appConfig: ApplicationConfig = {
  providers: [
    provideBrowserGlobalErrorListeners(),
    // HttpClient sends with fetch (the default) unless the mode asks otherwise; interceptors go in the list
    provideHttpClient(withInterceptors([]), ...MODE.http),
    ...MODE.providers,
  ],
};
