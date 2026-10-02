/** The review server's JSON API (python/server.py), typed. */

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
  speed_ms?: { gpu: number; cpu: number; browser: number };
  checks?: Record<string, [number, number]>;
}

export interface Models {
  chosen: string;
  device: 'cuda' | 'cpu';
  speed: string;
  checked_on: string;
  checks: { key: string; name: string; what: string; of?: number }[];
  models: Model[];
}

export async function getJson<T>(path: string, signal?: AbortSignal): Promise<T> {
  const r = await fetch(path, { signal });
  const body = await r.json();
  if (!r.ok) throw new Error(body?.error ?? r.statusText);
  return body as T;
}
