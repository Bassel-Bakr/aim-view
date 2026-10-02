import { HttpRequest } from '@angular/common/http';
import { HttpTestingController, TestRequest } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';
import { Recording } from './api';

/** An answer that changes from request to request: it gets the request. */
export type RouteHandler = (req: HttpRequest<unknown>) => unknown;

/** API paths and the JSON each one answers with, or a handler that makes it. */
export type ApiRoutes = Record<string, unknown>;

/** A response body, as a test request takes it. */
type Body = Parameters<TestRequest['flush']>[0];

/** An answer meaning the server is not there: the request fails without a response. */
export const NO_SERVER = Symbol('no server');

const settle = () => new Promise((r) => setTimeout(r));

/**
 * Answers the app's pending requests as the review server would: each path in routes with its JSON (or what its
 * handler returns), any other path with a 404. Repeats until no request is left, since answers start new ones.
 */
export async function answer(routes: ApiRoutes): Promise<void> {
  const http = TestBed.inject(HttpTestingController);
  for (let idle = 0; idle < 3;) {
    TestBed.tick();
    const pending = http.match(() => true);
    idle = pending.length ? 0 : idle + 1;
    for (const req of pending) {
      const path = req.request.url;
      if (!(path in routes)) {
        req.flush({ error: 'not found' }, { status: 404, statusText: 'Not Found' });
        continue;
      }
      const route = routes[path];
      const body = typeof route === 'function' ? (route as RouteHandler)(req.request) : route;
      if (body === NO_SERVER) req.error(new ProgressEvent('error'));
      else req.flush(body as Body);
    }
    await settle();
  }
}

/** A recording for tests: a reviewed static run with a stats file, unless overrides say otherwise. */
export function recording(overrides: Partial<Recording>): Recording {
  return {
    id: 'a/a - 1 - 2026.08.11-13.55.27.mp4',
    scenario: 'a',
    kind: 'static',
    score: 1,
    stamp: '2026.08.11-13.55.27',
    mtime: 0,
    size: 68e6,
    stats: true,
    analysed: true,
    not_aim: false,
    ...overrides,
  };
}
