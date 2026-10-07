/**
 * A fake review server for the UI's tests: it answers the app's pending HttpClient requests from
 * a table of API paths, and gives a test recording and the server mode's providers. In: a spec's
 * routes. Out: the specs, and the contract specs' modes (platform/contract-case.ts).
 */

import { HttpRequest, provideHttpClient } from '@angular/common/http';
import {
  HttpTestingController,
  provideHttpClientTesting,
  TestRequest,
} from '@angular/common/http/testing';
import { EnvironmentProviders, Provider } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { Recording } from './api';
import { MODE as SERVER } from './modes/mode.server';

/** An answer that changes from request to request: it gets the request. */
export type RouteHandler = (req: HttpRequest<unknown>) => unknown;

/** API paths and the JSON each one answers with, or a handler that makes it. */
export type ApiRoutes = Record<string, unknown>;

/** A response body, as a test request takes it. */
type Body = Parameters<TestRequest['flush']>[0];

/** An answer meaning the server is not there: the request fails without a response. */
export const NO_SERVER = Symbol('no server');

/** An answer refusing the request, as the review server does a request it cannot carry out: 400 and why. */
export class Refused {
  /** `error`: why the server refuses, sent as the body's error. */
  constructor(readonly error: string) {}
}

/**
 * An answer with a status of its own and a JSON body: the service asking for something first (409, need: found), or
 * any other failure with more than an error's text.
 */
export class Status {
  /** `status`: the HTTP status to answer with; `body`: the JSON body. */
  constructor(
    readonly status: number,
    readonly body: unknown,
  ) {}
}

/** Waits one turn of the event loop, so what an answer started can send its requests. */
const settle = () => new Promise((resolve) => setTimeout(resolve));

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
      else if (body instanceof Refused) {
        req.flush({ error: body.error }, { status: 400, statusText: 'Bad Request' });
      } else if (body instanceof Status) {
        req.flush(body.body as Body, { status: body.status, statusText: 'Error' });
      } else req.flush(body as Body);
    }
    await settle();
  }
}

/** Answers the fake review server's requests until the call ends. */
export async function served<T>(call: Promise<T>, routes: ApiRoutes): Promise<T> {
  let done = false;
  const end = () => (done = true);
  call.then(end, end);
  while (!done) await answer(routes);
  return call;
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

/** What a test of the server mode runs with: its services, and a fake review server that answer() answers. */
export function serverMode(): (Provider | EnvironmentProviders)[] {
  return [provideHttpClient(), provideHttpClientTesting(), ...SERVER.providers];
}
