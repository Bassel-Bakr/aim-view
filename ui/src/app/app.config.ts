import { provideHttpClient, withInterceptors } from '@angular/common/http';
import { ApplicationConfig, provideBrowserGlobalErrorListeners } from '@angular/core';
import { MODE } from './modes/mode';

export const appConfig: ApplicationConfig = {
  providers: [
    provideBrowserGlobalErrorListeners(),
    // HttpClient sends with fetch (the default) unless the mode asks otherwise; interceptors go in the list
    provideHttpClient(withInterceptors([]), ...MODE.http),
    ...MODE.providers,
  ],
};
