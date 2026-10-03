import { HttpInterceptorFn } from '@angular/common/http';

/**
 * Where the desktop app answers the review server's API (desktop/src/api.rs): its own protocol, in the app itself,
 * with no network port.
 */
export const DESKTOP_API = 'http://api.localhost';

/** Sends the server-mode services' requests (/api/...) to the app. */
export const desktopApi: HttpInterceptorFn = (req, next) =>
  next(req.url.startsWith('/api/') ? req.clone({ url: DESKTOP_API + req.url }) : req);
