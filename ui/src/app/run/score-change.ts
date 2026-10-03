import { Recording } from '../api';
import { formatNumber } from '../format';

/** A run's score against the same scenario's run before it: the change in words, and whether it went up. */
export interface ScoreChange {
  text: string;
  up: boolean;
}

/**
 * The change from the latest earlier run of the same scenario with a score, among the recordings listed; null with
 * none. Time stamps (yyyy.mm.dd-hh.mm.ss) sort as text.
 */
export function scoreChange(run: Recording, all: readonly Recording[]): ScoreChange | null {
  if (run.score === null) return null;
  let before: Recording | null = null;
  for (const r of all) {
    if (r.scenario !== run.scenario || r.score === null || r.stamp >= run.stamp) continue;
    if (!before || r.stamp > before.stamp) before = r;
  }
  if (before?.score == null) return null;
  const change = run.score - before.score;
  const sign = change >= 0 ? '+' : '−';
  return { text: `${sign}${formatNumber(Math.abs(change))} on the run before`, up: change >= 0 };
}
