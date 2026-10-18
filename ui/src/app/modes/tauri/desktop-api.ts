/**
 * Sends the page's API requests to the desktop app. In: each HttpClient request. Out: the /api ones
 * readdressed to the app's own protocol (desktop/src/protocol.rs); mode.desktop.ts installs it.
 */

import { HttpInterceptorFn } from '@angular/common/http';
import { from, switchMap } from 'rxjs';

/**
 * Where the desktop app answers the review server's API (service/, through
 * desktop/src/protocol.rs): its own protocol, in the app itself, with no network port. Windows'
 * webview (WebView2) reaches an app's protocol as http://<name>.localhost, Linux's (WebKitGTK) as
 * <name>://localhost.
 */
export const DESKTOP_API = navigator.userAgent.includes('Windows')
  ? 'http://api.localhost'
  : 'api://localhost';

/**
 * Sends the server-mode services' requests (/api/...) to the app. A Blob body (a file the user
 * picked, a part of one, a batch made in the page) goes as its bytes, read into memory first:
 * neither webview hands an app's protocol a Blob's body whole (WebView2 drops a picked file sent
 * whole, WebKitGTK every Blob), while bytes always arrive. Big files go in parts
 * (DesktopRecordings), so what is read here stays small.
 */
export const desktopApi: HttpInterceptorFn = (req, next) => {
  if (!req.url.startsWith('/api/')) return next(req);
  const url = DESKTOP_API + req.url;
  const body = req.body;
  if (!(body instanceof Blob)) return next(req.clone({ url }));
  return from(body.arrayBuffer()).pipe(switchMap((bytes) => next(req.clone({ url, body: bytes }))));
};
