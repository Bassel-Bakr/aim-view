/**
 * Test helpers for the contract specs (platform/*.spec.ts): the modes each spec runs against and
 * the set-up of one. In: each mode's providers and the fake review server (fake-api.ts). Out: the
 * specs, which run the same checks on every mode.
 */

import { provideHttpClient } from '@angular/common/http';
import { provideHttpClientTesting } from '@angular/common/http/testing';
import { EnvironmentProviders, Provider } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { ApiRoutes, served, serverMode } from '../fake-api';
import { MODE as BROWSER } from '../modes/mode.browser';

/**
 * A mode a contract spec runs against: its providers, and how to let a call finish (its calls wait for the fake
 * review server's answers: in browser mode, the review service in the page stands in for it).
 */
export interface ModeCase {
  /** The mode's name, in the spec's titles. */
  name: string;
  /** The providers a test of the mode needs. */
  providers: () => (Provider | EnvironmentProviders)[];
  /** Answers a call's requests from the routes until the call settles, and gives its result. */
  finish: <T>(call: Promise<T>, routes: ApiRoutes) => Promise<T>;
}

/**
 * The modes every contract spec runs against. The browser mode's requests to its review service (and its own files in
 * the service's mounts, /files/...) are answered by the fake review server too: the interceptor that sends them to the
 * worker is left out. The desktop mode is the server mode's services over another transport.
 */
export const MODE_CASES: ModeCase[] = [
  {
    name: 'browser',
    providers: () => [provideHttpClient(), provideHttpClientTesting(), ...BROWSER.providers],
    finish: served,
  },
  { name: 'server', providers: serverMode, finish: served },
];

/** Sets up a test of one mode, and gives back its service for the contract. */
export function setUp<T>(mode: ModeCase, contract: abstract new (...args: never[]) => T): T {
  TestBed.configureTestingModule({ providers: mode.providers() });
  return TestBed.inject(contract);
}
