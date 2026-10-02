import { EnvironmentProviders, Provider } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { answer, ApiRoutes, serverMode } from '../fake-api';
import { MODE as BROWSER } from '../modes/mode.browser';

/**
 * A mode a contract spec runs against: its providers, and how to let a call finish (the server mode's calls wait
 * for the fake review server's answers; the browser mode's need none).
 */
export interface ModeCase {
  name: string;
  providers: () => (Provider | EnvironmentProviders)[];
  finish: <T>(call: Promise<T>, routes: ApiRoutes) => Promise<T>;
}

/** Answers the fake review server's requests until the call ends. */
async function served<T>(call: Promise<T>, routes: ApiRoutes): Promise<T> {
  let done = false;
  const end = () => (done = true);
  call.then(end, end);
  while (!done) await answer(routes);
  return call;
}

/** The modes every contract spec runs against. Desktop runs the browser mode's services until desktop/ exists. */
export const MODE_CASES: ModeCase[] = [
  { name: 'browser', providers: () => BROWSER.providers, finish: (call) => call },
  { name: 'server', providers: serverMode, finish: served },
];

/** Sets up a test of one mode, and gives back its service for the contract. */
export function setUp<T>(mode: ModeCase, contract: abstract new (...args: never[]) => T): T {
  TestBed.configureTestingModule({ providers: mode.providers() });
  return TestBed.inject(contract);
}
