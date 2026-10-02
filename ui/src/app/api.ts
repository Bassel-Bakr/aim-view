/** The review server's JSON API (python/server.py), typed. */

/** Milliseconds per 1280 x 720 frame on each runtime. */
export interface ModelSpeed {
  gpu: number;
  cpu: number;
  browser: number;
}

/** A model's result on one check: [kills matched, flicks measured], or for tracking [mean, mean size]. */
export type CheckResult = [first: number, second: number];

export type CheckKey = 'static' | 'moving' | 'uploads' | 'tracking';

export type ModelChecks = Partial<Record<CheckKey, CheckResult>>;

export interface Model {
  name: string;
  label: string;
  available: boolean;
  default?: boolean;
  older?: boolean;
  params?: number;
  kb?: number | null;
  trained?: string;
  best?: string;
  weak?: string;
  speed_ms?: ModelSpeed;
  checks?: ModelChecks;
}

/** One of the checks every model was measured on. */
export interface Check {
  key: CheckKey;
  name: string;
  what: string;
  of?: number;
}

export type Device = 'cuda' | 'cpu';

export interface Models {
  chosen: string;
  device: Device;
  speed: string;
  checked_on: string;
  checks: Check[];
  models: Model[];
}

/** A scenario's kind, from the game's tags (review.scenario_kinds). */
export type Kind = 'static' | 'dynamic' | 'tracking' | 'switching';

/** A recording in the list (/api/vods). kind is null for a scenario the game no longer has. */
export interface Recording {
  id: string;
  scenario: string;
  kind: Kind | null;
  score: number | null;
  stamp: string;
  mtime: number;
  size: number;
  stats: boolean;
  analysed: boolean;
  not_aim: boolean;
  uploaded?: boolean;
}

/** The body of an error response. */
export interface ApiError {
  error?: string;
}

export async function getJson<T>(path: string, signal?: AbortSignal): Promise<T> {
  const r = await fetch(path, { signal });
  const body: T & ApiError = await r.json();
  if (!r.ok) throw new Error(body.error ?? r.statusText);
  return body;
}
