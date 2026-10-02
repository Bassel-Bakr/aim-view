import { Recording } from './api';

/** API paths and the JSON each one answers with. */
export type ApiRoutes = Record<string, unknown>;

/** A fetch that answers the paths in routes with their JSON, and any other path with a 404. */
export function fakeFetch(routes: ApiRoutes): typeof fetch {
  return async (input) => {
    const path = new URL(String(input), 'http://localhost').pathname;
    return path in routes
      ? new Response(JSON.stringify(routes[path]))
      : new Response(JSON.stringify({ error: 'not found' }), { status: 404 });
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
