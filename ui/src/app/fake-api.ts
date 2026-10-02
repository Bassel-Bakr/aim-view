import { Recording } from './api';

/** An answer that changes from call to call: it gets the request's URL and options. */
export type RouteHandler = (url: URL, init?: RequestInit) => unknown;

/** API paths and the JSON each one answers with, or a handler that makes it. */
export type ApiRoutes = Record<string, unknown>;

/** A fetch that answers the paths in routes with their JSON, and any other path with a 404. */
export function fakeFetch(routes: ApiRoutes): typeof fetch {
  return async (input, init) => {
    const url = new URL(String(input), 'http://localhost');
    if (!(url.pathname in routes)) {
      return new Response(JSON.stringify({ error: 'not found' }), { status: 404 });
    }
    const route = routes[url.pathname];
    const body = typeof route === 'function' ? (route as RouteHandler)(url, init) : route;
    return new Response(JSON.stringify(body));
  };
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
