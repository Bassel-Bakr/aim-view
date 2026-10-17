/**
 * Sends the page's API requests to the desktop app. In: each HttpClient request. Out: the /api ones
 * readdressed to the app's own protocol (desktop/src/protocol.rs); mode.desktop.ts installs it.
 */

import { HttpInterceptorFn } from '@angular/common/http';

/**
 * Where the desktop app answers the review server's API (service/, through
 * desktop/src/protocol.rs): its own protocol, in the app itself, with no network port.
 */
export const DESKTOP_API = 'http://api.localhost';

/**
 * Sends the server-mode services' requests (/api/...) to the app. A file the user picked goes as a
 * slice of itself: the window sends a picked file whole as a reference to it on disk, which never
 * reaches the app's protocol (it would get an empty body), while a slice goes as its bytes.
 */
export const desktopApi: HttpInterceptorFn = (req, next) => {
  if (!req.url.startsWith('/api/')) return next(req);
  const body =
    req.body instanceof File ? req.body.slice(0, req.body.size, req.body.type) : req.body;
  return next(req.clone({ url: DESKTOP_API + req.url, body }));
};
